//! Zig: build.zig.zon (declared; Zig object notation read as data with a
//! depth and size limit, build.zig is never run) plus `zig-out` (output),
//! `.zig-cache` (cache) and `zig-pkg` (packages fetched into the project
//! since Zig 0.16) beside build.zig or build.zig.zon. Versions come from a
//! tag or archive name in the URL or from the package hash; nothing is
//! fetched.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};

pub struct Zig;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "zig",
    name: "Zig",
    markers: &["build.zig.zon"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        "build.zig.zon (Name als String oder Enum-Literal, url/hash/path/lazy)",
        "zig-pkg (Ordnernamen = Paket-Hash)",
        "zig-out, .zig-cache",
    ],
    limits: "Kein zig-Aufruf; build.zig wird nicht ausgeführt; Versionen nur aus URL-Tag, Archivname oder Paket-Hash, sonst unbekannt; globaler Paketcache nicht gelesen.",
};

impl Adapter for Zig {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        bounded(manifest_items(file)?)
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        if !siblings.iter().any(|s| s == "build.zig" || s == "build.zig.zon") {
            return None;
        }
        let (category, label) = match name {
            "zig-out" => (ItemCategory::Output, "Zig-Buildausgabe"),
            ".zig-cache" | "zig-cache" => (ItemCategory::Cache, "Zig-Buildcache"),
            "zig-pkg" => (ItemCategory::Environment, "Zig-Pakete"),
            _ => return None,
        };
        Some(DirClaim { adapter: "zig", category, label })
    }

    fn inspect_dir(&self, claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
        let item = InventoryItem::new(claim.category, "zig", name, rel_path);
        match claim.category {
            ItemCategory::Output => Ok(vec![item.detail("Generierte Buildausgabe; Größe erst nach Messung bekannt")]),
            ItemCategory::Cache => Ok(vec![item.detail("Lokaler Buildcache; Inhalt nicht gelesen")]),
            _ => {
                let mut packages = Vec::new();
                let mut unnamed = 0;
                for (entry, is_dir) in access.list(&[]).into_iter().take(20_000) {
                    if !is_dir || entry.starts_with('.') {
                        continue;
                    }
                    let Some((package, version, digest)) = package_hash(&entry) else {
                        unnamed += 1;
                        continue;
                    };
                    let mut fetched = InventoryItem::new(ItemCategory::Package, "zig", package, rel_path)
                        .version(version)
                        .state(InstallState::Installed)
                        .detail("abgerufen laut Ordnername (Paket-Hash)");
                    fetched.evidence.qualifier = digest.to_string();
                    packages.push(fetched);
                }
                let mut detail = format!("{} lokal abgerufene Pakete", packages.len());
                if unnamed > 0 {
                    detail.push_str(&format!(", {unnamed} ohne Namen im Hash"));
                }
                detail.push_str("; nicht ausgeführt");
                let mut items = vec![item.detail(detail)];
                items.extend(packages);
                bounded(items)
            }
        }
    }
}

/// A value of the Zig object notation. Enum literals, numbers and other
/// bare words are kept as text; list items (`.paths`) are checked, not kept.
enum Zon {
    Struct(Vec<(String, Zon)>),
    List,
    Str(String),
    Word(String),
}

const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 100_000;
const MAX_STRING: usize = 8_192;

struct Parser<'a> {
    text: &'a str,
    pos: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn error<T>(&self, message: &str) -> Result<T, String> {
        let line = self.text.get(..self.pos).unwrap_or(self.text).matches('\n').count() + 1;
        Err(format!("build.zig.zon: {message} (Zeile {line})"))
    }

    /// Whitespace and `//` comments.
    fn skip(&mut self) {
        loop {
            while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
                self.pos += 1;
            }
            if !self.text[self.pos..].starts_with("//") {
                break;
            }
            while self.peek().is_some_and(|b| b != b'\n') {
                self.pos += 1;
            }
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.skip();
        let found = self.peek() == Some(byte);
        if found {
            self.pos += 1;
        }
        found
    }

    fn value(&mut self, depth: usize) -> Result<Zon, String> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return self.error("zu tief verschachtelt oder zu viele Werte");
        }
        self.skip();
        let rest = &self.text[self.pos..];
        if rest.starts_with(".{") {
            self.pos += 2;
            return self.init(depth);
        }
        if rest.starts_with('.') {
            self.pos += 1;
            return Ok(Zon::Word(self.identifier()?));
        }
        if rest.starts_with('"') {
            return Ok(Zon::Str(self.string()?));
        }
        if rest.starts_with("\\\\") {
            return Ok(Zon::Str(self.multiline()));
        }
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-')) {
            self.pos += 1;
        }
        if self.pos == start {
            return self.error("Wert erwartet");
        }
        Ok(Zon::Word(self.text[start..self.pos].to_string()))
    }

    /// After `.{`: a struct of `.name = value` fields or a list of values;
    /// an empty `.{}` counts as an empty struct.
    fn init(&mut self, depth: usize) -> Result<Zon, String> {
        self.skip();
        let fields = self.field_ahead() || self.peek() == Some(b'}');
        let mut members = Vec::new();
        loop {
            if self.eat(b'}') {
                break;
            }
            if fields {
                if !self.eat(b'.') {
                    return self.error("Feld erwartet");
                }
                let name = self.identifier()?;
                if !self.eat(b'=') {
                    return self.error("'=' erwartet");
                }
                members.push((name, self.value(depth + 1)?));
            } else {
                self.value(depth + 1)?;
            }
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                break;
            }
            return self.error("',' oder '}' erwartet");
        }
        Ok(if fields { Zon::Struct(members) } else { Zon::List })
    }

    /// Whether `.name =` or `.@"name" =` follows.
    fn field_ahead(&self) -> bool {
        let bytes = self.text.as_bytes();
        let mut p = self.pos;
        if bytes.get(p) != Some(&b'.') {
            return false;
        }
        p += 1;
        if bytes.get(p) == Some(&b'@') {
            p += 1;
            if bytes.get(p) != Some(&b'"') {
                return false;
            }
            p += 1;
            while let Some(&b) = bytes.get(p) {
                p += 1;
                match b {
                    b'\\' => p += 1,
                    b'"' => break,
                    b'\n' => return false,
                    _ => {}
                }
            }
        } else {
            let start = p;
            while bytes.get(p).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') {
                p += 1;
            }
            if p == start {
                return false;
            }
        }
        while bytes.get(p).is_some_and(|b| b.is_ascii_whitespace()) {
            p += 1;
        }
        bytes.get(p) == Some(&b'=') && bytes.get(p + 1) != Some(&b'=')
    }

    /// A bare or `@"quoted"` identifier.
    fn identifier(&mut self) -> Result<String, String> {
        if self.peek() == Some(b'@') {
            self.pos += 1;
            return self.string();
        }
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
            self.pos += 1;
        }
        if self.pos == start {
            return self.error("Name erwartet");
        }
        Ok(self.text[start..self.pos].to_string())
    }

    /// A string literal; `\x` and `\u{…}` escapes are not decoded.
    fn string(&mut self) -> Result<String, String> {
        if self.peek() != Some(b'"') {
            return self.error("Zeichenkette erwartet");
        }
        self.pos += 1;
        let mut out = String::new();
        let mut chars = self.text[self.pos..].char_indices();
        while let Some((offset, ch)) = chars.next() {
            let decoded = match ch {
                '"' => {
                    self.pos += offset + 1;
                    return Ok(out);
                }
                '\n' => break,
                '\\' => match chars.next() {
                    Some((_, 'n')) => '\n',
                    Some((_, 't')) => '\t',
                    Some((_, c @ ('"' | '\\' | '\''))) => c,
                    Some(_) => '?',
                    None => break,
                },
                other => other,
            };
            if out.len() < MAX_STRING {
                out.push(decoded);
            }
        }
        self.error("Zeichenkette nicht abgeschlossen")
    }

    /// The `\\` lines of a multiline string literal.
    fn multiline(&mut self) -> String {
        let mut out = String::new();
        while let Some(line) = self.text[self.pos..].strip_prefix("\\\\") {
            let end = line.find('\n').unwrap_or(line.len());
            if out.len() < MAX_STRING {
                out.push_str(&line[..end]);
                out.push('\n');
            }
            self.pos += 2 + end;
            let after = self.pos;
            while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
                self.pos += 1;
            }
            if !self.text[self.pos..].starts_with("\\\\") {
                self.pos = after;
                break;
            }
        }
        out
    }
}

fn parse(text: &str) -> Result<Zon, String> {
    let mut parser = Parser { text, pos: 0, nodes: 0 };
    let value = parser.value(0)?;
    parser.skip();
    if parser.pos < text.len() {
        return parser.error("Zeichen nach dem Ende");
    }
    Ok(value)
}

/// `name-version-hashplus` (Zig 0.14+): name, version and the start of the
/// 44 character digest. Legacy `1220…` and nameless `N-V-…` hashes give none.
fn package_hash(hash: &str) -> Option<(&str, &str, &str)> {
    let split = hash.len().checked_sub(44)?;
    let (prefix, digest) = (hash.get(..split)?, hash.get(split..)?);
    let (name, version) = prefix.strip_suffix('-')?.split_once('-')?;
    let identifier = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let base64 = digest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (identifier && base64 && !version.is_empty() && (name, version) != ("N", "V"))
        .then(|| (name, version, &digest[..12]))
}

fn version_like(text: &str) -> bool {
    let digits = text.strip_prefix(['v', 'V']).unwrap_or(text);
    // Length first: this runs on every suffix after a `-`.
    text.len() <= 64
        && digits.starts_with(|c: char| c.is_ascii_digit())
        && digits.contains('.')
        && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
}

/// A version from a tag (`?ref=v1.2.0`, `#v1.2.0`, `…/tags/v1.2.0.tar.gz`)
/// or an archive name (`lib-1.2.0.zip`), and a commit from `#<sha>` or an
/// archive named after one.
fn url_version(url: &str) -> (Option<String>, Option<String>) {
    let (rest, fragment) = url.split_once('#').map_or((url, None), |(rest, fragment)| (rest, Some(fragment)));
    let (rest, query) = rest.split_once('?').map_or((rest, None), |(rest, query)| (rest, Some(query)));
    let commit = fragment.and_then(super::swiftpm::commit).map(str::to_string);
    let reference = query
        .and_then(|q| q.split('&').find_map(|pair| pair.strip_prefix("ref=")))
        .or(fragment.filter(|_| commit.is_none()));
    if let Some(reference) = reference {
        let tag = reference.rsplit('/').next().unwrap_or(reference);
        return (version_like(tag).then(|| bare(tag).to_string()), commit);
    }
    if url.get(..4).is_some_and(|scheme| scheme.eq_ignore_ascii_case("git+")) {
        return (None, commit);
    }
    let name = rest.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    let extension = [".tar.gz", ".tar.xz", ".tar.zst", ".tar.bz2", ".tgz", ".tar", ".zip"]
        .iter()
        .find(|extension| lower.ends_with(*extension))
        .map_or(0, |extension| extension.len());
    let stem = &name[..name.len() - extension];
    if let Some(sha) = super::swiftpm::commit(stem) {
        return (None, Some(sha.to_string()));
    }
    if version_like(stem) {
        return (Some(bare(stem).to_string()), commit);
    }
    let version =
        stem.match_indices('-').map(|(index, _)| &stem[index + 1..]).find(|candidate| version_like(candidate));
    (version.map(|v| bare(v).to_string()), commit)
}

/// A tag such as `v1.2.0` names version `1.2.0`, written as in the package hash.
fn bare(tag: &str) -> &str {
    tag.strip_prefix(['v', 'V']).filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit())).unwrap_or(tag)
}

fn manifest_items(file: &FoundFile<'_>) -> AdapterResult {
    let Zon::Struct(fields) = parse(file.content)? else {
        return Err("build.zig.zon: kein Struct-Literal".into());
    };
    fn text(value: Option<&Zon>) -> Option<&str> {
        match value {
            Some(Zon::Str(text) | Zon::Word(text)) => Some(text.as_str()),
            _ => None,
        }
    }
    let field = |name: &str| fields.iter().find(|(key, _)| key == name).map(|(_, value)| value);
    let mut items = Vec::new();
    if let Some(Zon::Struct(dependencies)) = field("dependencies") {
        for (name, spec) in dependencies.iter().take(10_000) {
            let Zon::Struct(spec) = spec else { continue };
            let get = |key: &str| spec.iter().find(|(k, _)| k == key).map(|(_, value)| value);
            let lazy = matches!(get("lazy"), Some(Zon::Word(word)) if word == "true");
            let mut item = InventoryItem::new(ItemCategory::Package, "zig", name.as_str(), file.rel_path())
                .state(InstallState::Declared);
            let mut detail = if text(get("path")).is_some() {
                item = item.source(SourceClass::Path, None);
                "lokaler Pfad".to_string()
            } else if let Some(url) = text(get("url")) {
                let (source, host) = classify_url(url, &[]);
                let hash = text(get("hash")).unwrap_or_default();
                let (from_url, commit) = url_version(url);
                let from_hash = package_hash(hash).map(|(_, version, _)| version.to_string());
                let mut detail = match source {
                    SourceClass::Git => "Git".to_string(),
                    SourceClass::Path => "lokale Datei".to_string(),
                    _ => "Archiv-URL".to_string(),
                };
                let version = match (from_url, from_hash) {
                    (Some(version), _) => {
                        detail.push_str(" · Version aus URL");
                        version
                    }
                    (None, Some(version)) => {
                        detail.push_str(" · Version aus Paket-Hash");
                        version
                    }
                    (None, None) => {
                        detail.push_str(" · Version unbekannt");
                        item.evidence.qualifier = super::swiftpm::short(hash).to_string();
                        String::new()
                    }
                };
                if let Some(commit) = commit {
                    detail.push_str(&format!(" · Commit {commit}"));
                }
                item = item.source(source, host).version(version);
                detail
            } else {
                continue;
            };
            if lazy {
                detail.push_str(" · lazy (nur bei Bedarf geladen)");
            }
            items.push(item.detail(detail));
        }
    }
    let mut detail = match (text(field("name")), text(field("version"))) {
        (Some(name), Some(version)) => format!("Paket {name} {version}"),
        (Some(name), None) => format!("Paket {name}"),
        _ => "Paket ohne Namen".to_string(),
    };
    if let Some(minimum) = text(field("minimum_zig_version")) {
        detail.push_str(&format!(" · Zig-Mindestversion {minimum}"));
    }
    detail.push_str(&format!(" · {} Abhängigkeiten", items.len()));
    items.insert(0, manifest("zig", file, detail));
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const ZON: &str = r#".{
    // This is the default name used by packages depending on this one.
    .name = .my_project,
    .version = "0.0.1",
    .fingerprint = 0x9a3b4c5d6e7f8091, // Changing this has security and trust implications.
    .minimum_zig_version = "0.14.0",
    .dependencies = .{
        .zap = .{
            .url = "https://github.com/zigzap/zap/archive/refs/tags/v0.9.1.tar.gz",
            .hash = "zap-0.9.1-GoeB84M8JACXxNjuJ5mmPGZPNd5n-ypUPdgF0tZFS-Rx",
        },
        .known_folders = .{
            .url = "git+https://github.com/ziglibs/known-folders.git#0ad514dcfb7525e32ae349b9acc0a53976f3a9fa",
            .hash = "known_folders-0.0.0-Fy-PJtLDAADGDOwYwMkVydMSTp_aN-nfjCZw6qPQ2ECL",
            .lazy = true,
        },
        .private = .{
            .url = "https://ci:s3cr3t@git.corp.example/zig/private/archive/main.tar.gz",
            .hash = "1220eb0b5f3d6fa1e05c63f9da7e9ee2b0c0e2f8c2f5ec5e7f0dc5e71d3c1b2a3f4e",
        },
        .@"zig-clap" = .{
            .url = "git+https://github.com/Hejsil/zig-clap?ref=0.10.0#e47028deaefc2fb396d3d9e9f7bd776ae0b2a43a",
            .hash = "clap-0.10.0-oBajB434AQBDh-Ei3YtoKIRxZacVPF1iSwp3IX_ZB8f0",
        },
        .local_dep = .{
            .path = "libs/local_dep",
        },
    },
    .paths = .{
        "build.zig",
        "build.zig.zon",
        "src",
        "",
    },
}
"#;

    #[test]
    fn zon_dependencies_with_versions_from_urls_and_hashes() {
        let items = Zig.parse_file(&file("build.zig.zon", ZON)).unwrap();
        assert_eq!(items[0].detail, "Paket my_project 0.0.1 · Zig-Mindestversion 0.14.0 · 5 Abhängigkeiten");
        let find = |name: &str| items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name}"));
        let zap = find("zap");
        assert_eq!((zap.version.as_deref(), zap.source), (Some("0.9.1"), SourceClass::PrivateRegistry));
        assert_eq!(zap.detail, "Archiv-URL · Version aus URL");
        let folders = find("known_folders");
        assert_eq!((folders.version.as_deref(), folders.source), (Some("0.0.0"), SourceClass::Git));
        assert_eq!(
            folders.detail,
            "Git · Version aus Paket-Hash · Commit 0ad514dcfb75 · lazy (nur bei Bedarf geladen)"
        );
        let private = find("private");
        assert_eq!((private.version.as_deref(), private.evidence.qualifier.as_str()), (None, "1220eb0b5f3d"));
        assert_eq!(private.source_host.as_deref(), Some("git.corp.example"));
        let clap = find("zig-clap");
        assert_eq!(clap.version.as_deref(), Some("0.10.0"));
        assert_eq!(clap.detail, "Git · Version aus URL · Commit e47028deaefc");
        assert_eq!(find("local_dep").source, SourceClass::Path);
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Declared)));
        let debug = format!("{items:?}");
        assert!(!debug.contains("s3cr3t") && !debug.contains("libs/local_dep") && !debug.contains("PZPNd5n"));
    }

    #[test]
    fn legacy_string_names_and_archives_named_by_commit() {
        let legacy = ".{\n    .name = \"legacy\",\n    .version = \"0.1.0\",\n    .dependencies = .{\n        .mach = .{\n            .url = \"https://pkg.machengine.org/mach/fe1ffcdd7c4ab1ad77f42f3f0ffe6a1df5c3ec44.tar.gz\",\n            .hash = \"1220c4b0d4f37f7f1f2be36e3d1e0c3d1fa7fd5ab4a1c4b7b1f9e0a3cdd5c0c2a1b\",\n        },\n        .freetype = .{\n            .url = \"https://example.org/downloads/freetype-2.14.1.tar.xz\",\n        },\n    },\n}\n";
        let items = Zig.parse_file(&file("build.zig.zon", legacy)).unwrap();
        assert_eq!(items[0].detail, "Paket legacy 0.1.0 · 2 Abhängigkeiten");
        let mach = items.iter().find(|i| i.name == "mach").unwrap();
        assert_eq!(mach.version, None);
        assert!(mach.detail.ends_with("Commit fe1ffcdd7c4a"), "{}", mach.detail);
        assert_eq!(items.iter().find(|i| i.name == "freetype").unwrap().version.as_deref(), Some("2.14.1"));
        let empty = Zig
            .parse_file(&file(
                "build.zig.zon",
                ".{ .name = .empty, .version = \"0.0.0\", .dependencies = .{}, .paths = .{\"\"} }",
            ))
            .unwrap();
        assert_eq!(empty.len(), 1);
        assert_eq!(
            Zig.parse_file(&file("build.zig.zon", ".{}")).unwrap()[0].detail,
            "Paket ohne Namen · 0 Abhängigkeiten"
        );
    }

    #[test]
    fn broken_zon_is_an_error() {
        assert!(Zig.parse_file(&file("build.zig.zon", ".{ .name = \"x\", ")).is_err());
        assert!(Zig.parse_file(&file("build.zig.zon", ".{ .url = \"unterminated }")).is_err());
        assert!(Zig.parse_file(&file("build.zig.zon", "\"just a string\"")).is_err());
        let deep = format!("{}{}", ".{ .a = ".repeat(40), "1 }".repeat(40));
        assert!(Zig.parse_file(&file("build.zig.zon", &deep)).is_err());
    }

    #[test]
    fn output_cache_and_fetched_packages_beside_a_build_file() {
        assert!(Zig.claim_dir("zig-out", &["main.zig".into()], &[]).is_none());
        let siblings = ["build.zig".to_string()];
        let output = Zig.claim_dir("zig-out", &siblings, &[]).unwrap();
        assert_eq!(output.category, ItemCategory::Output);
        let cache = Zig.claim_dir(".zig-cache", &siblings, &[]).unwrap();
        let items = Zig.inspect_dir(cache, "app/.zig-cache", &mut FakeDir::default()).unwrap();
        assert_eq!((items[0].category, items[0].name.as_str()), (ItemCategory::Cache, ".zig-cache"));
        assert_eq!(Zig.claim_dir("zig-cache", &siblings, &[]).unwrap().category, ItemCategory::Cache);

        let packages = Zig.claim_dir("zig-pkg", &["build.zig.zon".to_string()], &[]).unwrap();
        let mut dir = FakeDir::default();
        dir.dirs.insert(
            String::new(),
            vec![
                ("zap-0.9.1-GoeB84M8JACXxNjuJ5mmPGZPNd5n-ypUPdgF0tZFS-Rx".into(), true),
                ("N-V-__8AAKBBAwDUyxEj1IKYvm1cM5BRBVpWkTwnR63uYrUe".into(), true),
                (".tmp".into(), true),
                ("notes.txt".into(), false),
            ],
        );
        let items = Zig.inspect_dir(packages, "zig-pkg", &mut dir).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].detail, "1 lokal abgerufene Pakete, 1 ohne Namen im Hash; nicht ausgeführt");
        let zap = &items[1];
        assert_eq!((zap.name.as_str(), zap.version.as_deref()), ("zap", Some("0.9.1")));
        assert_eq!(
            (zap.install_state, zap.evidence.qualifier.as_str()),
            (Some(InstallState::Installed), "GoeB84M8JACX")
        );
    }
}
