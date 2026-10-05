//! Strict option parsing: unknown, duplicate or incomplete options are
//! errors. No abbreviations, no implicit defaults for destructive commands.
use mogumogu::{Error, Result};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct Options {
    values: BTreeMap<String, Vec<String>>,
    flags: Vec<String>,
}

impl Options {
    /// Parses `--key value` pairs and bare `--flag`s. `repeatable` options
    /// may occur several times; everything else at most once.
    pub fn parse(args: &[String], with_value: &[&str], flags: &[&str], repeatable: &[&str]) -> Result<Self> {
        let mut options = Options::default();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if flags.contains(&arg.as_str()) {
                if options.flags.contains(arg) {
                    return Err(Error::invalid(format!("Option {arg} doppelt angegeben.")));
                }
                options.flags.push(arg.clone());
            } else if with_value.contains(&arg.as_str()) {
                let value = iter
                    .next()
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| Error::invalid(format!("Wert für {arg} fehlt.")))?;
                let entry = options.values.entry(arg.clone()).or_default();
                if !entry.is_empty() && !repeatable.contains(&arg.as_str()) {
                    return Err(Error::invalid(format!("Option {arg} doppelt angegeben.")));
                }
                entry.push(value.clone());
            } else {
                return Err(Error::invalid(format!("Unbekannte Option oder unerwartetes Argument: {arg}")));
            }
        }
        Ok(options)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }

    pub fn text(&self, name: &str) -> Result<String> {
        self.optional(name).ok_or_else(|| Error::invalid(format!("Option {name} fehlt.")))
    }

    pub fn optional(&self, name: &str) -> Option<String> {
        self.values.get(name).and_then(|v| v.first()).cloned()
    }

    pub fn id(&self, name: &str) -> Result<i64> {
        parse_id(&self.text(name)?, name)
    }

    pub fn optional_id(&self, name: &str) -> Result<Option<i64>> {
        self.optional(name).map(|v| parse_id(&v, name)).transpose()
    }

    pub fn ids(&self, name: &str) -> Result<Vec<i64>> {
        let values = self.values.get(name).cloned().unwrap_or_default();
        if values.is_empty() {
            return Err(Error::invalid(format!("Option {name} fehlt.")));
        }
        values.iter().map(|v| parse_id(v, name)).collect()
    }
}

fn parse_id(value: &str, name: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| Error::invalid(format!("{name} erwartet eine positive ganze Zahl.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn duplicates_and_unknowns_are_errors() {
        assert!(Options::parse(&args(&["--name", "a", "--name", "b"]), &["--name"], &[], &[]).is_err());
        assert!(Options::parse(&args(&["--force"]), &["--name"], &[], &[]).is_err());
        assert!(Options::parse(&args(&["--name"]), &["--name"], &[], &[]).is_err());
        let ok = Options::parse(&args(&["--resource", "1", "--resource", "2"]), &["--resource"], &[], &["--resource"])
            .unwrap();
        assert_eq!(ok.ids("--resource").unwrap(), vec![1, 2]);
        assert!(Options::parse(&args(&["--id", "-3"]), &["--id"], &[], &[]).unwrap().id("--id").is_err());
    }
}
