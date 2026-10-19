//! Lexical checks for registration metadata ONLY. These functions do not prove
//! file identity, containment or permission to read/delete any filesystem object.
use crate::{Error, Result};

pub fn name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 100 || value.chars().any(char::is_control) {
        return Err(Error::Invalid("Name: 1–100 Zeichen, ohne Steuerzeichen.".into()));
    }
    Ok(value.into())
}
pub fn windows_path(value: &str) -> Result<String> {
    let value = value.replace('/', "\\");
    let b = value.as_bytes();
    if b.len() < 4 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
        return Err(Error::Invalid("Ein lokaler absoluter Ordnerpfad wie C:\\dev\\projekt ist erforderlich.".into()));
    }
    if value.len() > 4096
        || value.chars().any(char::is_control)
        || value[3..].chars().any(|c| matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(Error::Invalid("Der Pfad enthält nicht unterstützte Zeichen.".into()));
    }
    let rest = value[3..].trim_end_matches('\\');
    if rest.is_empty()
        || rest
            .split('\\')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.ends_with(' ') || part.ends_with('.'))
    {
        return Err(Error::Invalid(
            "Keine Laufwerkswurzel, leeren Komponenten oder relativen Pfadsegmente registrieren.".into(),
        ));
    }
    let normalized = format!("{}:\\{}", (b[0] as char).to_ascii_uppercase(), rest);
    Ok(normalized)
}

/// A whole local drive such as `D:\`. Only a names-only project search may
/// use one; every other approval keeps the stricter [`windows_path`] rule.
pub fn drive_root(value: &str) -> Result<String> {
    let b = value.as_bytes();
    let separator = |c: &u8| matches!(c, b'\\' | b'/');
    if !(2..=3).contains(&b.len()) || !b[0].is_ascii_alphabetic() || b[1] != b':' || !b[2..].iter().all(separator) {
        return Err(Error::Invalid("Eine Laufwerkswurzel wie D:\\ ist erforderlich.".into()));
    }
    Ok(format!("{}:\\", (b[0] as char).to_ascii_uppercase()))
}
