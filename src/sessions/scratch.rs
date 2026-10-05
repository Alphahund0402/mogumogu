//! Managed scratchpad layout (PROJEKTPLAN §9.1): source, environment,
//! temporary output and results are separate folders from the start.
use crate::clock::{Timestamp, iso_utc};
use crate::domain::ResourceKind;
use crate::platform;
use crate::storage::ManagedFolder;
use crate::{Error, Result, validation};
use std::fs;
use std::path::Path;

pub const LAYOUT: &[(&str, ResourceKind)] = &[
    ("source", ResourceKind::Source),
    ("environment", ResourceKind::Environment),
    ("temporary", ResourceKind::Temporary),
    ("results", ResourceKind::Results),
];

/// `20261004-parser-spike`: date plus a conservative slug.
pub fn folder_name(name: &str, now: Timestamp) -> String {
    let date: String = iso_utc(now)[..10].chars().filter(|c| c.is_ascii_digit()).collect();
    let mut slug = String::new();
    for ch in name.chars().flat_map(char::to_lowercase) {
        let ch = match ch {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            'ß' => 's',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        };
        if !(ch == '-' && slug.ends_with('-')) {
            slug.push(ch);
        }
    }
    let slug = slug.trim_matches('-');
    let slug: String = slug.chars().take(40).collect();
    format!("{date}-{}", if slug.is_empty() { "scratch" } else { &slug })
}

/// Creates a fresh scratchpad below `root`. Existing folders are never
/// reused: an existing location would carry unknown content.
pub fn create_layout(root: &str, name: &str, now: Timestamp) -> Result<(ManagedFolder, Vec<ManagedFolder>)> {
    let root = validation::windows_path(root)?;
    fs::create_dir_all(&root)?;
    let base = Path::new(&root).join(folder_name(name, now));
    fs::create_dir(&base).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => {
            Error::conflict("Ein Scratchpad-Ordner mit diesem Namen existiert bereits.")
        }
        _ => Error::Io(e),
    })?;
    let folder = |path: &Path, kind| -> Result<ManagedFolder> {
        Ok(ManagedFolder { kind, path: path.to_string_lossy().into_owned(), identity: platform::path_identity(path)? })
    };
    let mut children = Vec::new();
    for (name, kind) in LAYOUT {
        let path = base.join(name);
        fs::create_dir(&path)?;
        children.push(folder(&path, *kind)?);
    }
    fs::write(
        base.join("README.mogumogu.txt"),
        "Von mogumogu angelegter Scratchpad.\r\nsource/ und results/ bleiben erhalten. temporary/ kann nach \
         abgeschlossener Session und ausdrücklicher Freigabe bereinigt werden.\r\n",
    )?;
    Ok((folder(&base, ResourceKind::Scratchpad)?, children))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_dated_and_safe() {
        assert_eq!(folder_name("Parser Spike: Ü/../x", 1_791_123_600), "20261004-parser-spike-u-x");
        assert_eq!(folder_name("!!!", 0), "19700101-scratch");
    }
}
