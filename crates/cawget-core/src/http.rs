//! HTTP plumbing: client construction, request headers, probing, error classification.

use crate::settings::Settings;
use crate::store::Job;
use reqwest::header::{self, HeaderName, HeaderValue};
use reqwest::{Client, RequestBuilder, StatusCode};
use std::error::Error as _;
use std::time::Duration;

/// Why a request failed: worth retrying or not.
#[derive(Debug, Clone)]
pub enum FetchError {
    Transient(String),
    Permanent(String),
}

impl FetchError {
    pub fn message(&self) -> &str {
        match self {
            FetchError::Transient(m) | FetchError::Permanent(m) => m,
        }
    }
}

/// What the server told us about the resource.
#[derive(Debug, Clone)]
pub struct Probe {
    pub size: Option<u64>,
    /// True only if the server answered the range probe with 206.
    pub ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_disposition: Option<String>,
    /// URL after redirects.
    pub final_url: String,
}

/// One HTTP client per distinct (CA bundle, timeout) setting. HTTP/1.1 only, so every segment
/// uses its own TCP connection; no automatic decompression, so byte ranges address the raw entity.
pub fn build_client(s: &Settings) -> Result<Client, String> {
    let mut b = Client::builder()
        .http1_only()
        .redirect(reqwest::redirect::Policy::limited(20))
        .connect_timeout(Duration::from_secs(s.connect_timeout))
        .read_timeout(Duration::from_secs(30)) // stall detection
        .pool_idle_timeout(Duration::from_secs(20));
    // Proxy: "" keeps the system proxy (environment variables), "none" connects directly.
    match s.proxy.as_str() {
        "" => {}
        "none" => b = b.no_proxy(),
        url => b = b.proxy(reqwest::Proxy::all(url).map_err(|e| format!("bad proxy {url}: {e}"))?),
    }
    if let Some(path) = &s.cacert {
        let pem = std::fs::read(path)
            .map_err(|e| format!("cannot read cacert {}: {e}", path.display()))?;
        let certs = reqwest::Certificate::from_pem_bundle(&pem)
            .map_err(|e| format!("bad cacert {}: {e}", path.display()))?;
        for c in certs {
            b = b.add_root_certificate(c);
        }
    }
    b.build()
        .map_err(|e| format!("cannot create HTTP client: {e}"))
}

/// `Authorization: Basic …` header line for a login and password (RFC 7617, UTF-8).
pub fn basic_auth_header(login: &str, password: &str) -> String {
    use base64::Engine;
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{login}:{password}"));
    format!("Authorization: Basic {token}")
}

/// Parses `Name: value`; used both to validate input and to build requests.
pub fn parse_header(line: &str) -> Option<(HeaderName, HeaderValue)> {
    let (n, v) = line.split_once(':')?;
    Some((
        HeaderName::from_bytes(n.trim().as_bytes()).ok()?,
        HeaderValue::from_str(v.trim()).ok()?,
    ))
}

/// A GET request carrying the job's referer, cookie, user agent and extra headers.
pub fn request(client: &Client, job: &Job, default_ua: &str, url: &str) -> RequestBuilder {
    let mut rb = client.get(url);
    rb = rb.header(
        header::USER_AGENT,
        job.user_agent.as_deref().unwrap_or(default_ua),
    );
    if let Some(r) = &job.referer {
        rb = rb.header(header::REFERER, r);
    }
    if let Some(c) = &job.cookie {
        rb = rb.header(header::COOKIE, c);
    }
    for line in &job.headers {
        if let Some((n, v)) = parse_header(line) {
            rb = rb.header(n, v);
        }
    }
    rb
}

fn error_chain(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        s.push_str(": ");
        s.push_str(&c.to_string());
        cur = c.source();
    }
    s
}

/// Maps a transport error to retry / give up. Certificate problems and bad URLs never get better.
pub fn classify_error(e: &reqwest::Error) -> FetchError {
    let msg = error_chain(e);
    let lower = msg.to_ascii_lowercase();
    if e.is_builder()
        || lower.contains("certificate")
        || lower.contains("unknownissuer")
        || lower.contains("invalid url")
    {
        FetchError::Permanent(msg)
    } else {
        FetchError::Transient(msg)
    }
}

/// `None` = acceptable status; otherwise the failure class (408, 429 and 5xx are transient).
pub fn classify_status(s: StatusCode) -> Option<FetchError> {
    if s.is_success() {
        return None;
    }
    let m = format!("HTTP {}", s.as_u16());
    Some(
        if s == StatusCode::REQUEST_TIMEOUT
            || s == StatusCode::TOO_MANY_REQUESTS
            || s.is_server_error()
        {
            FetchError::Transient(m)
        } else {
            FetchError::Permanent(m)
        },
    )
}

/// Total size from a `Content-Range` value (`bytes 0-0/1234` or `bytes */1234`).
pub fn content_range_total(v: &str) -> Option<u64> {
    v.rsplit_once('/')?.1.trim().parse().ok()
}

/// Start offset from a `Content-Range` value.
pub fn content_range_start(v: &str) -> Option<u64> {
    v.strip_prefix("bytes ")?
        .split('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

fn header_str(h: &header::HeaderMap, name: header::HeaderName) -> Option<String> {
    h.get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// Asks for byte 0 only. A 206 reveals size and range support; a 200 means ranges are ignored.
pub async fn probe(client: &Client, job: &Job, default_ua: &str) -> Result<Probe, FetchError> {
    let resp = request(client, job, default_ua, &job.url)
        .header(header::RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|e| classify_error(&e))?;
    let status = resp.status();
    let h = resp.headers();
    let cr = header_str(h, header::CONTENT_RANGE);
    let (size, ranges) = match status.as_u16() {
        206 => match cr.as_deref().and_then(content_range_total) {
            Some(t) => (Some(t), true),
            None => (None, false),
        },
        200 => (resp.content_length(), false),
        416 => (
            Some(cr.as_deref().and_then(content_range_total).unwrap_or(0)),
            false,
        ),
        _ => {
            return Err(classify_status(status)
                .unwrap_or_else(|| FetchError::Permanent(format!("HTTP {}", status.as_u16()))));
        }
    };
    let p = Probe {
        size,
        ranges,
        etag: header_str(h, header::ETAG),
        last_modified: header_str(h, header::LAST_MODIFIED),
        content_disposition: header_str(h, header::CONTENT_DISPOSITION),
        final_url: resp.url().to_string(),
    };
    drop(resp); // closes the connection; the body is not needed
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_parsing() {
        assert_eq!(content_range_total("bytes 0-0/12345"), Some(12345));
        assert_eq!(content_range_total("bytes */0"), Some(0));
        assert_eq!(content_range_total("bytes 0-0/*"), None);
        assert_eq!(content_range_start("bytes 100-199/1000"), Some(100));
        assert_eq!(content_range_start("items 1-2/3"), None);
    }

    #[test]
    fn status_classes() {
        assert!(classify_status(StatusCode::PARTIAL_CONTENT).is_none());
        assert!(matches!(
            classify_status(StatusCode::SERVICE_UNAVAILABLE),
            Some(FetchError::Transient(_))
        ));
        assert!(matches!(
            classify_status(StatusCode::TOO_MANY_REQUESTS),
            Some(FetchError::Transient(_))
        ));
        assert!(matches!(
            classify_status(StatusCode::NOT_FOUND),
            Some(FetchError::Permanent(_))
        ));
        assert!(matches!(
            classify_status(StatusCode::FORBIDDEN),
            Some(FetchError::Permanent(_))
        ));
    }

    #[test]
    fn basic_auth() {
        // RFC 7617 example
        assert_eq!(
            basic_auth_header("Aladdin", "open sesame"),
            "Authorization: Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
        assert!(parse_header(&basic_auth_header("ü", "p:w")).is_some());
    }

    #[test]
    fn header_lines() {
        assert!(parse_header("X-Test: abc").is_some());
        assert!(parse_header("no colon").is_none());
        assert!(parse_header("Bad Name: v").is_none());
    }
}
