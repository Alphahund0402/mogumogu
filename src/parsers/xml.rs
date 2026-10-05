//! XML without DTD, external entities or custom entity expansion.
use super::{ParseError, ParseResult};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    /// Local name without namespace prefix.
    pub name: String,
    pub attributes: Vec<(String, String)>,
    /// Direct text content, bounded.
    pub text: String,
    pub depth: usize,
    /// Local name of the parent element.
    pub parent: String,
}

impl Element {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attributes.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

fn local(start: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(start.local_name().as_ref()).into_owned()
}

fn attributes(start: &BytesStart<'_>) -> ParseResult<Vec<(String, String)>> {
    let mut out = Vec::new();
    for attribute in start.attributes().take(64) {
        let attribute = attribute.map_err(|_| ParseError::new("XML-Attribut ungültig"))?;
        let key = String::from_utf8_lossy(attribute.key.local_name().as_ref()).into_owned();
        // Only the five predefined entities are resolved; nothing external.
        let value =
            attribute.normalized_value(XmlVersion::Implicit1_0).map_err(|_| ParseError::new("XML-Wert ungültig"))?;
        out.push((key, crate::privacy::truncate(&value, 300)));
    }
    Ok(out)
}

/// Collects elements in document order. A DOCTYPE is rejected outright.
pub fn elements(text: &str, max_elements: usize, max_depth: usize) -> ParseResult<Vec<Element>> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut out: Vec<Element> = Vec::new();
    let mut stack: Vec<(String, usize)> = Vec::new();
    loop {
        if out.len() > max_elements {
            return Err(ParseError::new("XML-Elementlimit erreicht"));
        }
        let event = reader.read_event().map_err(|e| ParseError::new(format!("XML ungültig: {e}")))?;
        match event {
            Event::DocType(_) => return Err(ParseError::new("XML mit DOCTYPE wird nicht verarbeitet")),
            Event::Start(start) => {
                if stack.len() >= max_depth {
                    return Err(ParseError::new("XML-Verschachtelung zu tief"));
                }
                let parent = stack.last().map(|(n, _)| n.clone()).unwrap_or_default();
                out.push(Element {
                    name: local(&start),
                    attributes: attributes(&start)?,
                    text: String::new(),
                    depth: stack.len(),
                    parent,
                });
                stack.push((local(&start), out.len() - 1));
            }
            Event::Empty(start) => {
                let parent = stack.last().map(|(n, _)| n.clone()).unwrap_or_default();
                out.push(Element {
                    name: local(&start),
                    attributes: attributes(&start)?,
                    text: String::new(),
                    depth: stack.len(),
                    parent,
                });
            }
            Event::Text(text) => {
                if let Some((_, index)) = stack.last() {
                    let decoded = text.decode().map_err(|_| ParseError::new("XML-Text ungültig"))?;
                    let element = &mut out[*index];
                    if element.text.len() < 300 {
                        element.text.push_str(&crate::privacy::truncate(decoded.trim(), 300));
                    }
                }
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::Eof => return Ok(out),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctype_and_entities_are_rejected() {
        let evil = r#"<?xml version="1.0"?><!DOCTYPE x [<!ENTITY e SYSTEM "file:///c:/windows/win.ini">]><x>&e;</x>"#;
        assert!(elements(evil, 100, 8).is_err());
    }

    #[test]
    fn reads_attributes_text_and_parents() {
        let xml = r#"<Project><ItemGroup><PackageReference Include="Serilog" Version="3.1.1" /></ItemGroup><metadata><id>Foo</id></metadata></Project>"#;
        let items = elements(xml, 100, 8).unwrap();
        let reference = items.iter().find(|e| e.name == "PackageReference").unwrap();
        assert_eq!(reference.attr("include"), Some("Serilog"));
        assert_eq!(reference.parent, "ItemGroup");
        assert_eq!(items.iter().find(|e| e.name == "id").unwrap().text, "Foo");
    }
}
