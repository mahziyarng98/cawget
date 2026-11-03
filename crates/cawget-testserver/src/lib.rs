//! In-process HTTP/HTTPS test server (not part of the shipped program).
//!
//! Deterministic content: byte i of any resource = BLOCK[i % 1 MiB] ^ ((i >> 20) & 255).
//! Routes (`size` in bytes):
//!   /data/<size>?bps=N&slow0=N  Range + ETag; `bps` throttles each connection, `slow0` only
//!                               requests whose range starts at byte 0
//!   /norange/<size>             ignores Range (always 200)      /chunked/<size>   no Content-Length
//!   /redirect/<size>            302 to /data/<size>             /cd/<size>        Content-Disposition
//!   /flaky/<size>               first 2 requests answer 503     /ver/<size>       ETag changes with `bump()`
//!   /auth/<size>                like /data, but 401 unless "Authorization: Basic" for user "user", password "pa:ss wörd"
//!   anything else               404
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::stream;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const MIB: u64 = 1 << 20;

/// `Authorization` value the `/auth/` route accepts: user "user", password "pa:ss wörd" (UTF-8, base64).
pub const AUTH_OK: &str = "Basic dXNlcjpwYTpzcyB3w7ZyZA==";

fn block() -> &'static Vec<u8> {
    static B: OnceLock<Vec<u8>> = OnceLock::new();
    B.get_or_init(|| {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        (0..MIB)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    })
}

/// Bytes `start..=end` of the virtual file.
pub fn content(start: u64, end: u64) -> Vec<u8> {
    let b = block();
    (start..=end)
        .map(|i| b[(i % MIB) as usize] ^ ((i >> 20) & 255) as u8)
        .collect()
}

pub fn expected_sha(size: u64) -> String {
    let mut h = Sha256::new();
    let mut pos = 0;
    while pos < size {
        let n = (size - pos).min(4 * MIB);
        h.update(content(pos, pos + n - 1));
        pos += n;
    }
    hex(&h.finalize())
}

pub fn sha_file(path: &std::path::Path) -> String {
    let mut h = Sha256::new();
    let mut f = std::fs::File::open(path).unwrap();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf).unwrap();
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    hex(&h.finalize())
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, Default)]
pub struct Req {
    pub path: String,
    pub range: Option<String>,
    pub status: u16,
    pub bytes: u64,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub cookie: Option<String>,
    pub x_test: Option<String>,
    pub accept_encoding: Option<String>,
}

#[derive(Default)]
pub struct ServerState {
    log: Mutex<Vec<Req>>,
    now: AtomicUsize,
    peak: AtomicUsize,
    flaky: AtomicUsize,
    ver: AtomicUsize,
}

impl ServerState {
    pub fn log(&self) -> Vec<Req> {
        self.log.lock().unwrap().clone()
    }
    /// Range requests that carried file data (the `bytes=0-0` probe excluded).
    pub fn data_ranges(&self) -> Vec<Req> {
        self.log()
            .into_iter()
            .filter(|r| r.range.as_deref().is_some_and(|x| x != "bytes=0-0"))
            .collect()
    }
    pub fn bytes_served(&self) -> u64 {
        self.data_ranges().iter().map(|r| r.bytes).sum()
    }
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
    pub fn active(&self) -> usize {
        self.now.load(Ordering::SeqCst)
    }
    pub fn bump(&self) {
        self.ver.fetch_add(1, Ordering::SeqCst);
    }
    /// Waits for connections of a previous phase to end, then clears log and peak.
    pub async fn reset(&self) {
        for _ in 0..100 {
            if self.active() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.log.lock().unwrap().clear();
        self.peak.store(self.active(), Ordering::SeqCst);
    }
}

pub struct TestServer {
    pub base: String,
    pub state: Arc<ServerState>,
    /// PEM of the self-signed certificate (HTTPS server only).
    pub cert_pem: Option<String>,
}

impl TestServer {
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }
}

struct Tracker {
    state: Arc<ServerState>,
    idx: usize,
    sent: u64,
}

impl Drop for Tracker {
    fn drop(&mut self) {
        self.state.now.fetch_sub(1, Ordering::SeqCst);
        if let Some(r) = self.state.log.lock().unwrap().get_mut(self.idx) {
            r.bytes = self.sent;
        }
    }
}

fn app(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/{kind}/{size}", get(serve))
        // Same, with a file name at the end of the address (for tests that need an extension).
        .route("/{kind}/{size}/{name}", get(serve_named))
        .fallback(|| async { (StatusCode::NOT_FOUND, "not found") })
        .with_state(state)
}

async fn serve_named(
    st: State<Arc<ServerState>>,
    Path((kind, size, _name)): Path<(String, String, String)>,
    q: Query<HashMap<String, String>>,
    h: HeaderMap,
    u: axum::http::Uri,
) -> Response {
    serve(st, Path((kind, size)), q, h, u).await
}

fn parse_range(v: &str, size: u64) -> Option<(u64, u64)> {
    let r = v.strip_prefix("bytes=")?;
    let (a, b) = r.split_once('-')?;
    if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        return Some((size.saturating_sub(n), size.checked_sub(1)?));
    }
    let start: u64 = a.parse().ok()?;
    let end = if b.is_empty() {
        size.checked_sub(1)?
    } else {
        b.parse::<u64>().ok()?.min(size.checked_sub(1)?)
    };
    (start <= end && start < size).then_some((start, end))
}

async fn serve(
    State(st): State<Arc<ServerState>>,
    Path((kind, size)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    let Ok(size) = size.parse::<u64>() else {
        st.log.lock().unwrap().push(Req {
            path: uri.path().to_string(),
            status: 404,
            ..Default::default()
        });
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let h = |n: &str| {
        headers
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let mut rec = Req {
        path: uri.path().to_string(),
        range: h("range"),
        user_agent: h("user-agent"),
        referer: h("referer"),
        cookie: h("cookie"),
        x_test: h("x-test"),
        accept_encoding: h("accept-encoding"),
        ..Default::default()
    };
    let log = |st: &ServerState, mut r: Req, status: u16| -> usize {
        r.status = status;
        let mut l = st.log.lock().unwrap();
        l.push(r);
        l.len() - 1
    };

    match kind.as_str() {
        "data" | "norange" | "chunked" | "cd" | "flaky" | "ver" | "redirect" | "auth" => {}
        _ => {
            log(&st, rec, 404);
            return (StatusCode::NOT_FOUND, "not found").into_response();
        }
    }
    if kind == "redirect" {
        log(&st, rec, 302);
        return Response::builder()
            .status(302)
            .header(header::LOCATION, format!("/data/{size}"))
            .header(header::CONTENT_LENGTH, "0")
            .body(Body::empty())
            .unwrap();
    }
    if kind == "auth" && headers.get("authorization").and_then(|v| v.to_str().ok()) != Some(AUTH_OK)
    {
        log(&st, rec, 401);
        return Response::builder()
            .status(401)
            .header(header::WWW_AUTHENTICATE, "Basic realm=\"test\"")
            .header(header::CONTENT_LENGTH, "0")
            .body(Body::empty())
            .unwrap();
    }
    if kind == "flaky" && st.flaky.fetch_add(1, Ordering::SeqCst) < 2 {
        log(&st, rec, 503);
        return (StatusCode::SERVICE_UNAVAILABLE, "try later").into_response();
    }

    let ranged = kind != "norange" && kind != "chunked";
    let (mut start, mut end, mut status) = (0, size.saturating_sub(1), 200u16);
    if ranged && let Some(r) = &rec.range {
        match parse_range(r, size) {
            Some((a, b)) => {
                (start, end, status) = (a, b, 206);
            }
            None => {
                log(&st, rec, 416);
                return Response::builder()
                    .status(416)
                    .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                    .header(header::CONTENT_LENGTH, "0")
                    .body(Body::empty())
                    .unwrap();
            }
        }
    }
    let mut bps: u64 = q.get("bps").and_then(|v| v.parse().ok()).unwrap_or(0);
    if let Some(s0) = q.get("slow0").and_then(|v| v.parse::<u64>().ok())
        && start == 0
        && end > 0
    {
        bps = s0;
    }
    let length = if size == 0 { 0 } else { end - start + 1 };
    let ver = if kind == "ver" {
        st.ver.load(Ordering::SeqCst)
    } else {
        1
    };

    rec.status = status;
    let idx = log(&st, rec, status);
    st.now.fetch_add(1, Ordering::SeqCst);
    st.peak
        .fetch_max(st.now.load(Ordering::SeqCst), Ordering::SeqCst);
    let tracker = Tracker {
        state: Arc::clone(&st),
        idx,
        sent: 0,
    };

    let step: u64 = if bps > 0 { 16 * 1024 } else { 256 * 1024 };
    let body = Body::from_stream(stream::unfold(
        (tracker, start, length),
        move |(mut t, pos, left)| async move {
            if left == 0 {
                return None;
            }
            let n = left.min(step);
            let data = content(pos, pos + n - 1);
            if bps > 0 {
                tokio::time::sleep(Duration::from_secs_f64(n as f64 / bps as f64)).await;
            }
            t.sent += n;
            Some((
                Ok::<_, std::io::Error>(bytes::Bytes::from(data)),
                (t, pos + n, left - n),
            ))
        },
    ));

    let mut rb = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::ETAG, format!("\"seg-{size}-{ver}\""))
        .header(header::LAST_MODIFIED, "Wed, 21 Oct 2015 07:28:00 GMT");
    if ranged {
        rb = rb.header(header::ACCEPT_RANGES, "bytes");
    }
    if status == 206 {
        rb = rb.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    if kind == "cd" {
        rb = rb.header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"report final.bin\""),
        );
    }
    if kind != "chunked" {
        rb = rb.header(header::CONTENT_LENGTH, length.to_string());
    }
    rb.body(body).unwrap()
}

/// Plain HTTP server on a free local port.
pub async fn start() -> TestServer {
    let state = Arc::new(ServerState::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = app(Arc::clone(&state));
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    TestServer {
        base: format!("http://{addr}"),
        state,
        cert_pem: None,
    }
}

/// HTTPS server with a fresh self-signed certificate for `127.0.0.1`/`localhost`.
pub async fn start_tls() -> TestServer {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let state = Arc::new(ServerState::default());
    let cert =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .unwrap();
    let pem = cert.cert.pem();
    let cfg = axum_server::tls_rustls::RustlsConfig::from_pem(
        pem.clone().into_bytes(),
        cert.signing_key.serialize_pem().into_bytes(),
    )
    .await
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let router = app(Arc::clone(&state));
    tokio::spawn(async move {
        axum_server::from_tcp_rustls(listener, cfg)
            .unwrap()
            .serve(router.into_make_service())
            .await
            .unwrap();
    });
    TestServer {
        base: format!("https://{addr}"),
        state,
        cert_pem: Some(pem),
    }
}
