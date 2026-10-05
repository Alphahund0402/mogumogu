//! Markdown frontmatter: only top-level `key: value` pairs are extracted.
//! Nested YAML, anchors and tags are ignored, never interpreted.

/// Splits `---` frontmatter from the body. Returns `(pairs, body)`.
pub fn split(text: &str) -> (Vec<(String, String)>, &str) {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return (Vec::new(), text);
    };
    let Some(end) = find_end(rest) else { return (Vec::new(), text) };
    let (header, body) = rest.split_at(end);
    let body = body.trim_start_matches("---").trim_start_matches(['\r', '\n']);
    (pairs(header), body)
}

fn find_end(rest: &str) -> Option<usize> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n').take(200) {
        if line.trim_end() == "---" {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

fn pairs(header: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut folding: Option<usize> = None;
    for line in header.lines().take(200) {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if indented {
            if let Some(index) = folding {
                let value = &mut out[index].1;
                if !value.is_empty() {
                    value.push(' ');
                }
                value.push_str(line.trim());
            }
            continue;
        }
        folding = None;
        let Some((key, value)) = line.split_once(':') else { continue };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            continue;
        }
        let value = value.trim();
        if matches!(value, ">" | "|" | ">-" | "|-") {
            out.push((key.to_string(), String::new()));
            folding = Some(out.len() - 1);
        } else {
            out.push((key.to_string(), unquote(value).to_string()));
        }
    }
    out
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
}

pub fn get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_and_folded_values() {
        let text = "---\nname: demo-skill\ndescription: >\n  Erste Zeile\n  zweite\nmetadata:\n  version: \"1\"\n---\n# Body\n";
        let (pairs, body) = split(text);
        assert_eq!(get(&pairs, "name"), Some("demo-skill"));
        assert_eq!(get(&pairs, "description"), Some("Erste Zeile zweite"));
        assert!(body.starts_with("# Body"));
    }

    #[test]
    fn missing_frontmatter_is_not_an_error() {
        let (pairs, body) = split("# Nur Text");
        assert!(pairs.is_empty());
        assert_eq!(body, "# Nur Text");
    }
}
