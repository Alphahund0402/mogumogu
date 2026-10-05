//! Line-oriented formats: `key = value` files and a deliberately tiny YAML
//! subset (block mappings by indentation) for lockfiles. No YAML features
//! such as anchors, aliases, tags or flow collections are interpreted.

/// `key = value` pairs (pyvenv.cfg, .npmrc). Comments start with `#` or `;`.
pub fn key_values(text: &str) -> Vec<(String, String)> {
    text.lines()
        .take(10_000)
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

/// One `key:` line of a block-mapping YAML document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlLine<'a> {
    pub indent: usize,
    pub key: &'a str,
    /// Inline scalar after the colon (may be empty).
    pub value: &'a str,
}

/// Iterates mapping keys with their indentation. Lines that are not simple
/// `key:` mappings (lists, flow syntax, comments) are skipped.
pub fn yaml_keys(text: &str) -> impl Iterator<Item = YamlLine<'_>> {
    text.lines().take(2_000_000).filter_map(|line| {
        let trimmed = line.trim_start_matches(' ');
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            return None;
        }
        let indent = line.len() - trimmed.len();
        let (key, value) = split_key(trimmed)?;
        Some(YamlLine { indent, key: unquote(key), value: unquote(value.trim()) })
    })
}

fn split_key(line: &str) -> Option<(&str, &str)> {
    if let Some(rest) = line.strip_prefix('\'').or_else(|| line.strip_prefix('"')) {
        let quote = line.as_bytes()[0] as char;
        let end = rest.find(quote)?;
        let after = rest[end + 1..].strip_prefix(':')?;
        return Some((&line[..end + 2], after));
    }
    let index = line.find(": ").or_else(|| line.strip_suffix(':').map(|s| s.len()))?;
    Some((&line[..index], line.get(index + 1..).unwrap_or("")))
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('\'')
        .and_then(|v| v.strip_suffix('\''))
        .or_else(|| value.strip_prefix('"').and_then(|v| v.strip_suffix('"')))
        .unwrap_or(value)
}

/// Keys directly below a top-level section (`packages:` → its entries).
pub fn section_keys<'a>(text: &'a str, section: &str) -> Vec<YamlLine<'a>> {
    let mut inside = false;
    let mut child_indent: Option<usize> = None;
    let mut out = Vec::new();
    for line in yaml_keys(text) {
        if line.indent == 0 {
            inside = line.key == section;
            child_indent = None;
            continue;
        }
        if !inside {
            continue;
        }
        let indent = *child_indent.get_or_insert(line.indent);
        if line.indent == indent {
            out.push(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_values_skip_comments() {
        let pairs = key_values("# c\nhome = C:\\Python\nversion = 3.12.1\n");
        assert_eq!(pairs[1], ("version".into(), "3.12.1".into()));
    }

    #[test]
    fn section_keys_reads_direct_children_only() {
        let text = "lockfileVersion: '9.0'\npackages:\n\n  left-pad@1.3.0:\n    resolution: {integrity: x}\n  '@scope/pkg@2.0.0':\n    resolution: {}\nsnapshots:\n  other@1.0.0: {}\n";
        let keys: Vec<&str> = section_keys(text, "packages").iter().map(|l| l.key).collect();
        assert_eq!(keys, vec!["left-pad@1.3.0", "@scope/pkg@2.0.0"]);
    }
}
