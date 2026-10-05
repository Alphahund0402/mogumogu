//! Bounded, side-effect-free parsers for static discovery (F-07).
//!
//! Inputs are already size-limited by `fsread`. Parsers never execute,
//! import, expand variables, resolve entities or fetch anything; nesting is
//! checked before structured parsing.
pub mod frontmatter;
pub mod json;
pub mod lines;
pub mod xml;

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ParseError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(crate::privacy::label(&message.into()))
    }
}

pub type ParseResult<T> = Result<T, ParseError>;

/// Parses TOML into a table. TOML has no includes or code execution.
pub fn toml_table(text: &str) -> ParseResult<toml::Table> {
    text.parse::<toml::Table>().map_err(|e| ParseError::new(format!("TOML ungültig: {}", e.message())))
}
