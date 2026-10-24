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
    long_tokens(&known_secrets(text), mixed)
}

/// Credentials in URLs, query strings, known token formats and values of
/// secret-looking keys.
fn known_secrets(text: &str) -> String {
    let text = URL_USERINFO.replace_all(text, "$1***@");
    let text = URL_QUERY.replace_all(&text, "$1?…");
    let text = KNOWN_TOKENS.replace_all(&text, REDACTED);
    // Bearer first: "Authorization: Bearer x" must lose the token itself.
    let text = BEARER.replace_all(&text, |c: &Captures<'_>| format!("{} {REDACTED}", &c[1]));
    KEY_VALUE.replace_all(&text, |c: &Captures<'_>| format!("{}{}{REDACTED}", &c[1], &c[2])).into_owned()
}

/// Replaces each long run of token characters that `secret` takes for one.
fn long_tokens(text: &str, secret: impl Fn(&str) -> bool) -> String {
    LONG_TOKEN
        .replace_all(text, |c: &Captures<'_>| if secret(&c[0]) { REDACTED.to_string() } else { c[0].to_string() })
        .into_owned()
}

fn mixed(token: &str) -> bool {
    token.chars().any(|ch| ch.is_ascii_digit()) && token.chars().any(|ch| ch.is_ascii_alphabetic())
}

/// One folder or file name: words joined by `-`, `_` or `+`. Only a long
/// mixed word counts as a token, and none of hex digits in one case: that
/// is how commit ids, content hashes and GUIDs appear in archive, cache and
/// build folder names. Known token formats are redacted as in free text.
fn redact_name(name: &str) -> String {
    let hex = |word: &str| {
        word.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
            || word.chars().all(|c| matches!(c, '0'..='9' | 'A'..='F'))
    };
    long_tokens(&known_secrets(name), |run| {
        run.split(['-', '_', '+']).any(|word| word.len() >= 32 && mixed(word) && !hex(word))
    })
}

/// Redacted, single-line, bounded text for storage and display.
pub fn label(text: &str) -> String {
    bounded(&redact(text))
}

/// Like [`label`] for a `/`-separated relative path, redacted name by name.
/// A name cannot contain `/`, so a token never spans folders: an ordinary
/// path without dots is not one long secret, a token inside a name still is.
pub fn path(rel: &str) -> String {
    bounded(&rel.split('/').map(redact_name).collect::<Vec<_>>().join("/"))
}

fn bounded(redacted: &str) -> String {
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
    fn paths_are_redacted_name_by_name() {
        // As free text this whole path would be one long mixed token.
        let plain = "Astra_Private/Builds/TK008/tool-uv-cache/archive-v0/Xq_YCoooAy-0BXDyh4gsD";
        assert_eq!(path(plain), plain);
        assert_eq!(path("dev/ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/app"), format!("dev/{REDACTED}/app"));
        assert_eq!(path("dev/token=abc123secretvalue"), format!("dev/token={REDACTED}"));
        assert!(path(&"a/".repeat(300)).chars().count() <= LABEL_MAX_CHARS);
    }

    #[test]
    fn hashes_and_joined_words_in_names_are_no_tokens() {
        for name in [
            "scratch/actual-profile-identity-candidate-80",
            "builds/boundary-preserved-2026-10-06-26008e4e",
            "tools/llama.cpp-74d4f5b041ad837153b0e90fc864b8290e01d8d5",
            "extracted/cgltf-bbeb5b0b070ddacddac6852fb72143eb68454937",
            "store/7F3A9C2B4D5E6F708192A3B4C5D6E7F8091A2B3C",
            "cache/{3f2504e0-4f89-11d3-9a0c-0305e82c3301}",
            "runs/astra-bootstrap-controlled-188367804554000",
        ] {
            assert_eq!(path(name), name);
        }
        // A long word mixing cases, letters and digits still reads as a token,
        // in free text a hash stays redacted. Built at run time, so secret
        // scanners do not take the fixture for a real key.
        let mixed = format!("{}{}", "AbCd".repeat(6), "0123456789");
        assert_eq!(path(&format!("dev/{mixed}")), format!("dev/{REDACTED}"));
        assert_eq!(path("dev/sk-proj-0123456789abcdefABCDEF"), format!("dev/{REDACTED}"));
        assert_eq!(redact("commit 74d4f5b041ad837153b0e90fc864b8290e01d8d5"), format!("commit {REDACTED}"));
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
