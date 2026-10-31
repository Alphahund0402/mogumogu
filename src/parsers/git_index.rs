//! Git's index (`.git/index`), read only to tell whether a folder holds
//! versioned files. Versions 2–4 with SHA-1 or SHA-256 names. A split or
//! sparse index lists only part of the tracked files, so it is refused
//! rather than half read. Paths are compared case-insensitively, as Windows
//! resolves them; nothing of the index is kept.
use super::{ParseError, ParseResult};
use crate::domain::fold_case;
use std::collections::HashMap;

/// More entries than any repository this tool is meant for.
const MAX_ENTRIES: u32 = 4_000_000;
/// Git itself limits paths far below this.
const MAX_PATH: usize = 64 << 10;
/// Fixed part of an entry before the object name: times, ids, mode, size.
const STAT_BYTES: usize = 40;

/// Length of object names: 32 bytes with `extensions.objectFormat = sha256`
/// in the repository's config, otherwise SHA-1's 20.
pub fn hash_len(config: &str) -> usize {
    let mut section = String::new();
    for line in config.lines().take(100_000) {
        let mut line = line.trim();
        if let Some(rest) = line.strip_prefix('[') {
            let Some((name, after)) = rest.split_once(']') else { continue };
            section = name.trim().to_ascii_lowercase();
            // Git also allows `[extensions] objectformat = sha256` on one line.
            line = after.trim();
        }
        if section != "extensions" {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.split(['#', ';']).next().unwrap_or_default().trim().trim_matches('"');
        if key.trim().eq_ignore_ascii_case("objectformat") && value.eq_ignore_ascii_case("sha256") {
            return 32;
        }
    }
    20
}

/// For each of `wanted` (`/`-separated, relative to the work tree), the
/// number of index entries at or below it. `wanted` may be in any case.
pub fn tracked_below(index: &[u8], hash_len: usize, wanted: &[String]) -> ParseResult<Vec<u32>> {
    tracked_below_checked(index, hash_len, wanted, true)
}

/// As [`tracked_below`]; `checksum` false skips the trailing hash of an
/// index this process already checked (same file, same size and time).
pub fn tracked_below_checked(
    index: &[u8],
    hash_len: usize,
    wanted: &[String],
    checksum: bool,
) -> ParseResult<Vec<u32>> {
    if !matches!(hash_len, 20 | 32) {
        return Err(ParseError::new("Unbekannte Länge der Git-Objektnamen"));
    }
    // Several wanted paths may fold alike; each gets its count.
    let mut positions: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, path) in wanted.iter().enumerate() {
        positions.entry(fold_case(path.trim_matches('/'))).or_default().push(index);
    }
    let mut counts = vec![0_u32; wanted.len()];
    let mut at = Cursor { data: index, at: 0 };
    if at.take(4)? != b"DIRC" {
        return Err(ParseError::new("Keine Git-Indexdatei"));
    }
    let version = at.u32()?;
    if !(2..=4).contains(&version) {
        return Err(ParseError::new(format!("Git-Index-Version {version} wird nicht unterstützt")));
    }
    let entries = at.u32()?;
    let smallest = STAT_BYTES + hash_len + 3;
    if entries > MAX_ENTRIES || (entries as usize).saturating_mul(smallest) > index.len() {
        return Err(ParseError::new("Git-Index zu groß oder abgeschnitten"));
    }
    let mut previous: Vec<u8> = Vec::new();
    // A split index keeps placeholders without a name; its `link` extension
    // comes last and names the real reason.
    let mut nameless = false;
    for _ in 0..entries {
        let start = at.at;
        let stat = at.take(STAT_BYTES + hash_len)?;
        let mode = u32::from_be_bytes([stat[24], stat[25], stat[26], stat[27]]);
        let flags = at.u16()?;
        let extended = flags & 0x4000 != 0;
        if extended {
            if version < 3 {
                return Err(ParseError::new("Erweiterter Eintrag in Git-Index-Version 2"));
            }
            at.take(2)?;
        }
        let path = if version == 4 {
            // Version 4 stores how much of the previous path to drop, then the rest.
            let strip = at.varint()?;
            let suffix = at.until_nul()?;
            let keep = previous.len().checked_sub(strip).ok_or_else(|| ParseError::new("Git-Index beschädigt"))?;
            let mut path = previous[..keep].to_vec();
            path.extend_from_slice(suffix);
            path
        } else {
            let name = at.until_nul()?.to_vec();
            // Entries are padded with NULs to a multiple of eight bytes.
            let fixed = STAT_BYTES + hash_len + 2 + if extended { 2 } else { 0 };
            at.seek(start + ((fixed + name.len() + 8) & !7))?;
            name
        };
        // In every version the flags hold the full length of shorter paths.
        let length = usize::from(flags & 0xfff);
        if length != 0xfff && length != path.len() {
            return Err(ParseError::new("Git-Index beschädigt (Namenslänge)"));
        }
        if path.len() > MAX_PATH {
            return Err(ParseError::new("Git-Index beschädigt (Pfad)"));
        }
        nameless |= path.is_empty();
        if mode & 0o170000 == 0o040000 {
            return Err(ParseError::new("Sparse-Index: nicht alle versionierten Dateien sind aufgeführt"));
        }
        if !positions.is_empty() {
            count(&fold_case(&String::from_utf8_lossy(&path)), &positions, &mut counts);
        }
        previous = path;
    }
    // Extensions follow until the trailing checksum.
    while index.len() - at.at > hash_len {
        let signature = at.take(4)?;
        let size = at.u32()? as usize;
        match signature {
            b"link" => return Err(ParseError::new("Geteilter Git-Index (split index)")),
            b"sdir" => return Err(ParseError::new("Sparse-Index: nicht alle versionierten Dateien sind aufgeführt")),
            // Git itself refuses an unknown extension that is not marked optional.
            _ if !signature[0].is_ascii_uppercase() => {
                return Err(ParseError::new("Unbekannte Pflichterweiterung im Git-Index"));
            }
            _ => at.take(size)?,
        };
    }
    if index.len() - at.at != hash_len {
        return Err(ParseError::new("Git-Index abgeschnitten"));
    }
    if checksum && !checksum_matches(index, hash_len) {
        return Err(ParseError::new("Git-Index beschädigt (Prüfsumme)"));
    }
    if nameless {
        return Err(ParseError::new("Git-Index beschädigt (Eintrag ohne Pfad)"));
    }
    Ok(counts)
}

/// The trailing hash covers everything before it. With `index.skipHash`
/// Git writes zeros instead; then the structure checks alone decide.
fn checksum_matches(index: &[u8], hash_len: usize) -> bool {
    use sha2::{Digest, Sha256};
    let (content, stored) = index.split_at(index.len() - hash_len);
    if stored.iter().all(|b| *b == 0) {
        return true;
    }
    match hash_len {
        20 => super::sha1::sha1(content) == stored,
        _ => Sha256::digest(content).as_slice() == stored,
    }
}

/// Counts `path` for itself and every folder it lies in.
fn count(path: &str, positions: &HashMap<String, Vec<usize>>, counts: &mut [u32]) {
    let folders = path.match_indices('/').map(|(i, _)| &path[..i]);
    for prefix in folders.chain([path]) {
        for &index in positions.get(prefix).into_iter().flatten() {
            counts[index] = counts[index].saturating_add(1);
        }
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> ParseResult<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|end| *end <= self.data.len());
        let end = end.ok_or_else(|| ParseError::new("Git-Index abgeschnitten"))?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }

    fn u32(&mut self) -> ParseResult<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u16(&mut self) -> ParseResult<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn seek(&mut self, to: usize) -> ParseResult<()> {
        if to < self.at || to > self.data.len() {
            return Err(ParseError::new("Git-Index beschädigt (Eintragsgrenze)"));
        }
        self.at = to;
        Ok(())
    }

    /// Bytes up to the next NUL, which is consumed.
    fn until_nul(&mut self) -> ParseResult<&'a [u8]> {
        let rest = &self.data[self.at..];
        let length = rest.iter().take(MAX_PATH + 1).position(|b| *b == 0);
        let length = length.ok_or_else(|| ParseError::new("Git-Index beschädigt (Pfadende)"))?;
        let bytes = &rest[..length];
        self.at += length + 1;
        Ok(bytes)
    }

    /// Git's offset encoding: each continuation adds one before shifting.
    fn varint(&mut self) -> ParseResult<usize> {
        let mut byte = self.take(1)?[0];
        let mut value = usize::from(byte & 0x7f);
        while byte & 0x80 != 0 {
            byte = self.take(1)?[0];
            value = value
                .checked_add(1)
                .and_then(|v| v.checked_mul(128))
                .map(|v| v + usize::from(byte & 0x7f))
                .filter(|v| *v <= MAX_PATH)
                .ok_or_else(|| ParseError::new("Git-Index beschädigt (Längenangabe)"))?;
        }
        Ok(value)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal, structurally valid index with the given paths.
    pub(crate) fn index(version: u32, hash_len: usize, paths: &[&str], extensions: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut out = b"DIRC".to_vec();
        out.extend(version.to_be_bytes());
        out.extend((paths.len() as u32).to_be_bytes());
        let mut previous = "";
        for path in paths {
            let start = out.len();
            let mut stat = [0_u8; STAT_BYTES];
            stat[24..28].copy_from_slice(&0o100644_u32.to_be_bytes());
            out.extend(stat);
            out.extend(vec![0xab; hash_len]);
            out.extend((path.len().min(0xfff) as u16).to_be_bytes());
            if version == 4 {
                let common = previous.bytes().zip(path.bytes()).take_while(|(a, b)| a == b).count();
                let strip = previous.len() - common;
                assert!(strip < 128, "test helper writes one-byte lengths");
                out.push(strip as u8);
                out.extend(&path.as_bytes()[common..]);
                out.push(0);
            } else {
                out.extend(path.as_bytes());
                let fixed = STAT_BYTES + hash_len + 2;
                out.resize(start + ((fixed + path.len() + 8) & !7), 0);
            }
            previous = path;
        }
        for (signature, data) in extensions {
            out.extend(*signature);
            out.extend((data.len() as u32).to_be_bytes());
            out.extend(*data);
        }
        let hash = match hash_len {
            20 => super::super::sha1::sha1(&out).to_vec(),
            _ => <sha2::Sha256 as sha2::Digest>::digest(&out).to_vec(),
        };
        out.extend(hash);
        out
    }

    fn wanted(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn counts_entries_at_and_below_folders_in_all_versions() {
        let paths = ["dist/app.js", "dist/assets/logo.svg", "src/main.rs", "web/Dist/x.js", "distribution/a"];
        for version in [2, 3, 4] {
            for hash in [20, 32] {
                let bytes = index(version, hash, &paths, &[(b"TREE", b"cached tree")]);
                let counts =
                    tracked_below(&bytes, hash, &wanted(&["dist", "web/dist", "node_modules", "src/main.rs", "dis"]))
                        .unwrap();
                assert_eq!(counts, [2, 1, 0, 1, 0], "v{version}, {hash}-byte names");
            }
        }
    }

    #[test]
    fn partial_or_damaged_indexes_are_refused() {
        let good = index(2, 20, &["a/b.txt"], &[]);
        assert!(tracked_below(&good, 20, &[]).is_ok());
        assert!(tracked_below(&good[..good.len() - 1], 20, &[]).is_err(), "checksum cut off");
        assert!(tracked_below(&good, 32, &[]).is_err(), "wrong name length shows as damage");
        let mut version = good.clone();
        version[7] = 5;
        assert!(tracked_below(&version, 20, &[]).is_err());
        let split = index(2, 20, &["a/b.txt"], &[(b"link", &[0; 20])]);
        assert!(tracked_below(&split, 20, &[]).unwrap_err().0.contains("split"));
        // Git writes nameless placeholders into a split index.
        let placeholder = index(2, 20, &[""], &[(b"link", &[0; 20])]);
        assert!(tracked_below(&placeholder, 20, &[]).unwrap_err().0.contains("split"));
        assert!(tracked_below(&index(2, 20, &[""], &[]), 20, &[]).unwrap_err().0.contains("ohne Pfad"));
        let sparse = index(2, 20, &["a/b.txt"], &[(b"sdir", &[])]);
        assert!(tracked_below(&sparse, 20, &[]).unwrap_err().0.contains("Sparse"));
        let mut overcount = good.clone();
        overcount[11] = 200;
        assert!(tracked_below(&overcount, 20, &[]).is_err());
        assert!(tracked_below(b"DIRX", 20, &[]).is_err());
        let mut flipped = good.clone();
        flipped[12 + STAT_BYTES + 20 + 2] ^= 0x20; // `a/b.txt` becomes `A/b.txt`
        assert!(tracked_below(&flipped, 20, &[]).unwrap_err().0.contains("Prüfsumme"));
        let mut skip_hash = flipped.clone();
        let end = skip_hash.len();
        skip_hash[end - 20..].fill(0);
        assert!(tracked_below(&skip_hash, 20, &[]).is_ok(), "index.skipHash writes zeros");
        let mandatory = index(2, 20, &["a/b.txt"], &[(b"zzzz", b"new")]);
        assert!(tracked_below(&mandatory, 20, &[]).unwrap_err().0.contains("Pflichterweiterung"));
        let mut v4 = index(4, 20, &["a/b.txt", "a/c.txt"], &[]);
        let flags = 12 + STAT_BYTES + 20;
        v4[flags + 1] = 3; // the first entry claims a shorter path
        assert!(tracked_below(&v4, 20, &[]).is_err(), "version 4 checks the length too");
    }

    #[test]
    fn paths_fold_by_character_and_alike_wanted_paths_each_count() {
        let bytes = index(2, 20, &["ΟΔΟΣ/dist/a.js", "web/dist/b.js"], &[]);
        let counts = tracked_below(&bytes, 20, &wanted(&["οδοσ/dist", "Web/Dist", "web/dist"])).unwrap();
        assert_eq!(counts, [1, 1, 1]);
    }

    #[test]
    fn the_object_format_comes_from_the_extensions_section() {
        assert_eq!(hash_len("[core]\n\tbare = false\n"), 20);
        assert_eq!(hash_len("[core]\n[extensions]\n\tobjectformat = sha256\n"), 32);
        assert_eq!(hash_len("[Extensions] objectFormat = \"SHA256\" ; comment\n"), 32);
        assert_eq!(hash_len("[core]\n\tobjectformat = sha256\n"), 20, "only in [extensions]");
        assert_eq!(hash_len("[extensions]\n\tobjectformat = sha1\n"), 20);
    }
}
