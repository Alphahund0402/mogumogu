//! JSON and JSONC with a nesting limit checked before parsing.
use super::{ParseError, ParseResult};
use serde_json::Value;

/// Parses strict JSON after verifying the nesting depth.
pub fn parse(text: &str, max_depth: usize) -> ParseResult<Value> {
    check_depth(text, max_depth)?;
    serde_json::from_str(text).map_err(|e| ParseError::new(format!("JSON ungültig (Zeile {})", e.line())))
}

/// Parses JSON with comments and trailing commas (VS Code style JSONC).
pub fn parse_jsonc(text: &str, max_depth: usize) -> ParseResult<Value> {
    parse(&strip_jsonc(text), max_depth)
}

fn check_depth(text: &str, max_depth: usize) -> ParseResult<()> {
    let (mut depth, mut in_string, mut escaped) = (0_usize, false, false);
    for ch in text.chars() {
        if in_string {
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => {
                depth += 1;
                if depth > max_depth {
                    return Err(ParseError::new(format!("Verschachtelung über {max_depth} Ebenen – nicht gelesen")));
                }
            }
            '}' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// Removes `//` and `/* */` comments and trailing commas outside strings.
fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut in_string, mut escaped) = (0, false, false);
    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            out.push(ch);
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match (ch, chars.get(i + 1)) {
            ('"', _) => {
                in_string = true;
                out.push(ch);
                i += 1;
            }
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            (',', _) => {
                let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
                if !matches!(next, Some('}') | Some(']')) {
                    out.push(ch);
                }
                i += 1;
            }
            _ => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_nesting_is_rejected_before_parsing() {
        let deep = "[".repeat(40) + &"]".repeat(40);
        assert!(parse(&deep, 32).is_err());
        assert!(parse("[[1]]", 32).is_ok());
    }

    #[test]
    fn brackets_in_strings_do_not_count() {
        let text = r#"{"a": "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[["}"#;
        assert!(parse(text, 4).is_ok());
    }

    #[test]
    fn jsonc_comments_and_trailing_commas() {
        let text = "{ // comment\n \"a\": \"//not\", /* block */ \"b\": [1,2,], }";
        let value = parse_jsonc(text, 8).unwrap();
        assert_eq!(value["a"], "//not");
        assert_eq!(value["b"].as_array().unwrap().len(), 2);
    }
}
