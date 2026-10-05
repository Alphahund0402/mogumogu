//! Data minimisation (Implementierungsplan F-07, PROJEKTPLAN §7.3).
//!
//! Every free text that reaches SQLite, logs, IPC or exports passes through
//! [`label`] or [`redact`]. This is a first barrier with known limits: it
//! cannot recognise every secret hidden in arbitrary names, so the stored
//! fields are minimal by design and raw file content is never persisted.
use crate::limits::LABEL_MAX_CHARS;
use regex::{Captures, Regex};
use std::sync::LazyLock;

pub const REDACTED: &str = "[redigiert]";

static URL_USERINFO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s:@]+(?::[^/\s@]*)?@").expect("valid regex"));
static URL_QUERY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(\b[a-z][a-z0-9+.-]*://[^\s?#]+)[?#]\S*").expect("valid regex"));
static KNOWN_TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{20,}",
        r"|xox[abprs]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9_-]{16,}|sk_(?:live|test)_[A-Za-z0-9]{16,}",
        r"|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}|npm_[A-Za-z0-9]{36}",
        r"|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]*)"
    ))
    .expect("valid regex")
});
static KEY_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"(?i)\b(password|passwd|pwd|secret|token|api[_-]?key|apikey|access[_-]?key"#,
        r#"|client[_-]?secret|private[_-]?key|authorization|auth[_-]?token)"#,
        r#"(\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s,;&]+)"#
    ))
    .expect("valid regex")
});
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}").expect("valid regex"));
static LONG_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/_-]{32,}={0,2}").expect("valid regex"));

/// Remove credentials, tokens and query strings from free text.
pub fn redact(text: &str) -> String {
    let text = URL_USERINFO.replace_all(text, "$1***@");
    let text = URL_QUERY.replace_all(&text, "$1?…");
    let text = KNOWN_TOKENS.replace_all(&text, REDACTED);
    // Bearer first: "Authorization: Bearer x" must lose the token itself.
    let text = BEARER.replace_all(&text, |c: &Captures<'_>| format!("{} {REDACTED}", &c[1]));
    let text = KEY_VALUE.replace_all(&text, |c: &Captures<'_>| format!("{}{}{REDACTED}", &c[1], &c[2]));
    LONG_TOKEN
        .replace_all(&text, |c: &Captures<'_>| {
            let token = &c[0];
            let mixed = token.chars().any(|ch| ch.is_ascii_digit()) && token.chars().any(|ch| ch.is_ascii_alphabetic());
            if mixed { REDACTED.to_string() } else { token.to_string() }
        })
        .into_owned()
}

/// Redacted, single-line, bounded text for storage and display.
pub fn label(text: &str) -> String {
    let redacted = redact(text);
    let mut out = String::with_capacity(redacted.len().min(LABEL_MAX_CHARS * 4));
    let mut last_space = false;
    for ch in redacted.chars() {
        let ch = if ch.is_control() || ch.is_whitespace() { ' ' } else { ch };
        if ch == ' ' && last_space {
            continue;
        }
        last_space = ch == ' ';
        out.push(ch);
    }
    truncate(out.trim(), LABEL_MAX_CHARS)
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Host part of a URL without user info, port, path or query.
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    let valid = !host.is_empty()
        && host.len() <= 253
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    valid.then(|| host.to_ascii_lowercase())
}

/// Names of environment variables or headers whose values are never read.
pub fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["key", "token", "secret", "pass", "auth", "cred", "cookie", "session", "private"]
        .iter()
        .any(|needle| key.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_and_tokens_are_removed() {
        let samples = [
            "https://alice:hunter2@registry.example.com/npm/",
            "token=abc123secretvalue",
            "Authorization: Bearer abcdefghijklmnop.qrstuv",
            "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
            "AKIAABCDEFGHIJKLMNOP",
            "sk-proj-0123456789abcdefABCDEF",
            "https://example.com/hook?key=deadbeef",
            "API_KEY=\"x y z\"",
        ];
        let secrets = [
            "hunter2",
            "abc123secretvalue",
            "abcdefghijklmnop",
            "ABCDEFGHIJKLMNOPQRST",
            "AKIAABCDEFGHIJKLMNOP",
            "0123456789abcdef",
            "deadbeef",
            "x y z",
        ];
        for (sample, secret) in samples.iter().zip(secrets) {
            let out = redact(sample);
            assert!(!out.contains(secret), "{sample} -> {out}");
        }
    }

    #[test]
    fn ordinary_text_survives() {
        assert_eq!(redact("npm · TypeScript · 160 Pakete"), "npm · TypeScript · 160 Pakete");
        assert_eq!(redact("token expired"), "token expired");
    }

    #[test]
    fn labels_are_single_line_and_bounded() {
        let long = "a\nb\t".repeat(200);
        let out = label(&long);
        assert!(!out.contains('\n'));
        assert!(out.chars().count() <= LABEL_MAX_CHARS);
    }

    #[test]
    fn hosts_drop_credentials_and_ports() {
        assert_eq!(url_host("https://u:p@Registry.NPMJS.org:443/x?y").as_deref(), Some("registry.npmjs.org"));
        assert_eq!(url_host("not a url"), None);
        assert!(is_sensitive_key("GITHUB_TOKEN"));
        assert!(!is_sensitive_key("NODE_ENV"));
    }
}
