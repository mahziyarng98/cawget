//! Keeping credentials out of everything that is shown, printed or logged.
//!
//! * [`mask_url`] masks the userinfo of one URL (`user:***@host`, or `***@host` when only a
//!   user name, which may itself be a token, is given).
//! * [`redact_text`] masks the userinfo of every URL inside a free-form text (error messages,
//!   events), so a message never carries a password even if a library quoted a URL.
//! * [`redact_header`] keeps a header's name and hides its value.

/// Characters that end the authority part of a URL inside free-form text.
fn ends_authority(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '/' | '?' | '#' | '"' | '\'' | '<' | '>' | ')' | ']' | ','
        )
}

/// Masks the userinfo of the authority that starts at the beginning of `rest` (just after
/// `://`). Returns the masked authority and the number of bytes of `rest` it covered.
fn mask_authority(rest: &str) -> (String, usize) {
    // `]` is part of an IPv6 host, but only after the `@`; the userinfo itself cannot hold it.
    let end = rest
        .char_indices()
        .find(|&(i, c)| ends_authority(c) && !(c == ']' && rest[..i].contains('[')))
        .map_or(rest.len(), |(i, _)| i);
    let authority = &rest[..end];
    match authority.rfind('@') {
        None => (authority.to_string(), end),
        Some(at) => {
            let userinfo = &authority[..at];
            let host = &authority[at + 1..];
            let masked = match userinfo.split_once(':') {
                Some((user, _)) if !user.is_empty() => format!("{user}:***@{host}"),
                _ => format!("***@{host}"),
            };
            (masked, end)
        }
    }
}

/// Masks the userinfo of a URL. A URL without userinfo is returned unchanged.
pub fn mask_url(url: &str) -> String {
    match url.find("://") {
        Some(i) => {
            let (auth, used) = mask_authority(&url[i + 3..]);
            format!("{}{auth}{}", &url[..i + 3], &url[i + 3 + used..])
        }
        None => url.to_string(),
    }
}

/// Masks the userinfo of every `scheme://user:password@host` found in a text.
pub fn redact_text(text: &str) -> String {
    if !text.contains("://") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("://") {
        out.push_str(&rest[..i + 3]);
        let (auth, used) = mask_authority(&rest[i + 3..]);
        out.push_str(&auth);
        rest = &rest[i + 3 + used..];
    }
    out.push_str(rest);
    out
}

/// `Name: ***` for a `Name: value` header line (the name is not secret, the value may be).
pub fn redact_header(line: &str) -> String {
    match line.split_once(':') {
        Some((name, _)) => format!("{}: ***", name.trim()),
        None => "***".into(),
    }
}

/// Debug helper: a list of header lines with their values hidden.
pub(crate) struct RedactedHeaders<'a>(pub &'a [String]);

impl std::fmt::Debug for RedactedHeaders<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|h| redact_header(h)))
            .finish()
    }
}

/// Debug helper: an optional secret shown only as present or absent.
pub(crate) fn opt_secret(v: &Option<String>) -> &'static str {
    match v {
        Some(_) => "Some(***)",
        None => "None",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_url_hides_passwords_and_keeps_everything_else() {
        assert_eq!(
            mask_url("https://alice:S3cret@example.org:8443/a/b?x=1#f"),
            "https://alice:***@example.org:8443/a/b?x=1#f"
        );
        assert_eq!(mask_url("http://token@h/f"), "http://***@h/f");
        assert_eq!(mask_url("http://:pw@h/f"), "http://***@h/f");
        assert_eq!(
            mask_url("http://u:p%40ss@[::1]:80/p"),
            "http://u:***@[::1]:80/p"
        );
        // No userinfo: unchanged, also with '@' in the path or query.
        for u in [
            "http://example.org/a@b?c=d@e",
            "https://[::1]:8080/x",
            "http://h/p%20q",
            "not a url",
        ] {
            assert_eq!(mask_url(u), u);
        }
    }

    #[test]
    fn redact_text_masks_every_url() {
        let t =
            "error sending request for url (http://u:pw1@h/f): proxy http://p:pw2@px:3128 failed";
        let r = redact_text(t);
        assert!(!r.contains("pw1") && !r.contains("pw2"), "{r}");
        assert_eq!(
            r,
            "error sending request for url (http://u:***@h/f): proxy http://p:***@px:3128 failed"
        );
        assert_eq!(redact_text("HTTP 401"), "HTTP 401");
        assert_eq!(redact_text("see http://h/a@b"), "see http://h/a@b");
    }

    #[test]
    fn redact_header_keeps_only_the_name() {
        assert_eq!(
            redact_header("Authorization: Basic eHh4"),
            "Authorization: ***"
        );
        assert_eq!(redact_header("garbage"), "***");
    }
}
