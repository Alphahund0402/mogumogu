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

/// One line of a block-style YAML document: a mapping key, a list item or
/// both (`- name: x`). Block scalar content (`|`, `>`) is skipped and flow
/// collections stay raw text (see [`flow_list`]). Anchors, aliases, tags,
/// merge keys and escapes inside quotes are never interpreted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlEntry<'a> {
    /// Column of the key or scalar, after any `- ` list markers.
    pub indent: usize,
    /// The line opens a list item.
    pub item: bool,
    pub key: Option<&'a str>,
    /// Inline scalar without quotes or trailing comment; may be empty.
    pub value: &'a str,
}

/// Entries of a block-style YAML document, line by line. Comments, document
/// markers and directives are skipped.
pub fn yaml_entries(text: &str) -> impl Iterator<Item = YamlEntry<'_>> {
    // Lines deeper than this column belong to a block scalar.
    let mut block: Option<usize> = None;
    text.lines().take(2_000_000).filter_map(move |line| {
        let content = line.trim_start_matches(' ');
        let mut indent = line.len() - content.len();
        if content.is_empty() {
            return None;
        }
        if let Some(level) = block {
            if indent > level {
                return None;
            }
            block = None;
        }
        let marker = |m: &str| content == m || content.strip_prefix(m).is_some_and(|r| r.starts_with(' '));
        if content.starts_with('#') || content.starts_with('%') || marker("---") || marker("...") {
            return None;
        }
        let mut rest = content;
        let mut item = false;
        let mut dash = indent;
        while let Some(after) = rest.strip_prefix('-').filter(|a| a.is_empty() || a.starts_with(' ')) {
            item = true;
            dash = indent;
            let trimmed = after.trim_start_matches(' ');
            indent += rest.len() - trimmed.len();
            rest = trimmed;
        }
        let (key, raw) = match split_key(rest).filter(|_| !rest.starts_with(['{', '['])) {
            Some((key, value)) => (Some(unquote(key.trim_end())), value),
            None => (None, rest),
        };
        let value = scalar(raw);
        let indicator = value.strip_prefix(['|', '>']);
        if indicator.is_some_and(|flags| flags.chars().all(|c| matches!(c, '-' | '+' | '0'..='9'))) {
            block = Some(if key.is_some() { indent } else { dash });
        }
        Some(YamlEntry { indent, item, key, value })
    })
}

/// An inline scalar: the text inside quotes, or up to a ` #` comment.
fn scalar(raw: &str) -> &str {
    let raw = raw.trim();
    if let Some(quote) = raw.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        return raw[1..].find(quote).map_or(raw, |end| &raw[1..1 + end]);
    }
    if raw.starts_with('#') {
        return "";
    }
    raw.find(" #").map_or(raw, |index| raw[..index].trim_end())
}

/// Items of a one-line flow sequence such as `[push, "pull_request"]`.
/// Nested collections are not split; anything else is `None`.
pub fn flow_list(value: &str) -> Option<Vec<&str>> {
    let inner = value.trim().strip_prefix('[')?.strip_suffix(']')?;
    if inner.contains(['[', '{']) {
        return None;
    }
    Some(inner.split(',').map(|s| unquote(s.trim())).filter(|s| !s.is_empty()).collect())
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

    #[test]
    fn yaml_entries_read_lists_and_mappings_but_skip_block_scalars() {
        let text = "# top\nname: app\ndependencies:\n  - python=3.11  # pinned\n  - pip:\n    - requests==2.31\n  - \"numpy\"\n  - {name: x}\n  - -1\njobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4\n      - run: |\n          uses: not/an-action@v1\n\n          echo\n      - name: x\n        uses: 'actions/setup-node@v4' # pinned\nscript:\n  - |\n    uses: also/not@v1\n  - echo two\n---\nflow: [a, 'b']\nempty: # nothing\n";
        let entries: Vec<(usize, bool, Option<&str>, &str)> =
            yaml_entries(text).map(|e| (e.indent, e.item, e.key, e.value)).collect();
        assert_eq!(
            entries,
            vec![
                (0, false, Some("name"), "app"),
                (0, false, Some("dependencies"), ""),
                (4, true, None, "python=3.11"),
                (4, true, Some("pip"), ""),
                (6, true, None, "requests==2.31"),
                (4, true, None, "numpy"),
                (4, true, None, "{name: x}"),
                (4, true, None, "-1"),
                (0, false, Some("jobs"), ""),
                (2, false, Some("test"), ""),
                (4, false, Some("steps"), ""),
                (8, true, Some("uses"), "actions/checkout@v4"),
                (8, true, Some("run"), "|"),
                (8, true, Some("name"), "x"),
                (8, false, Some("uses"), "actions/setup-node@v4"),
                (0, false, Some("script"), ""),
                (4, true, None, "|"),
                (4, true, None, "echo two"),
                (0, false, Some("flow"), "[a, 'b']"),
                (0, false, Some("empty"), ""),
            ]
        );
        assert_eq!(flow_list("[a, 'b', \"c\"]"), Some(vec!["a", "b", "c"]));
        assert_eq!(flow_list("[a, [b]]"), None);
        assert_eq!(flow_list("a"), None);
    }
}
