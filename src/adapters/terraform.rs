//! Terraform / OpenTofu: `.terraform.lock.hcl` (resolved providers),
//! `*.tf` / `*.tofu` (declared providers and modules) and the provider
//! folders below `.terraform` (installed, folder names only). HCL is
//! tokenised with depth and size limits and never evaluated: variables,
//! overrides and `*.tf.json` stay unknown. State files, `*.tfvars` and
//! files inside `.terraform` are never read; nothing is initialised. A
//! `.terragrunt-cache` beside terragrunt.hcl is claimed, not walked.
use super::nix::{git_host, short_ref, valid_host};
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory, SourceClass};

pub struct Terraform;

const PUBLIC: &[&str] = &["registry.terraform.io", "registry.opentofu.org"];
const MAX_DEPTH: usize = 64;
const MAX_TOKENS: usize = 4_000_000;
/// Folder listings below `.terraform/providers`.
const MAX_LISTINGS: usize = 2_000;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "terraform",
    name: "Terraform/OpenTofu",
    markers: &[".terraform.lock.hcl", "*.tf", "*.tofu"],
    scope: AdapterScope::Project,
    inventory: Support::Experimental,
    cleanup: Support::Unsupported,
    formats: &[
        ".terraform.lock.hcl (Provider)",
        "*.tf, *.tofu (required_providers, module, required_version)",
        ".terraform/providers (Ordnernamen)",
    ],
    limits: "Kein terraform- oder tofu-Aufruf; HCL wird nicht ausgewertet – Variablen, Overrides und *.tf.json bleiben unbekannt; terraform.tfstate, *.tfvars und Dateien in .terraform werden nie gelesen; das Lockfile nennt keine Module.",
};

impl Adapter for Terraform {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn parse_file(&self, file: &FoundFile<'_>) -> AdapterResult {
        if file.name.eq_ignore_ascii_case(".terraform.lock.hcl") {
            bounded(lockfile(file)?)
        } else {
            bounded(configuration(file))
        }
    }

    fn claim_dir(&self, name: &str, siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        let configured = siblings.iter().any(|s| {
            let lower = s.to_ascii_lowercase();
            lower == ".terraform.lock.hcl" || lower.ends_with(".tf") || lower.ends_with(".tofu")
        });
        let terragrunt = siblings.iter().any(|s| s.eq_ignore_ascii_case("terragrunt.hcl"));
        match name {
            ".terraform" if configured => {
                Some(DirClaim { adapter: "terraform", category: ItemCategory::Environment, label: ".terraform" })
            }
            // Working copies of the configuration; walking them would declare everything twice.
            ".terragrunt-cache" if terragrunt => {
                Some(DirClaim { adapter: "terraform", category: ItemCategory::Cache, label: ".terragrunt-cache" })
            }
            _ => None,
        }
    }

    fn inspect_dir(&self, claim: DirClaim, rel_path: &str, access: &mut dyn DirAccess) -> AdapterResult {
        if claim.label == ".terragrunt-cache" {
            return Ok(vec![
                InventoryItem::new(ItemCategory::Cache, "terraform", claim.label, rel_path)
                    .detail("Terragrunt-Arbeitskopien; Inhalt nicht gelesen"),
            ]);
        }
        let mut items = Vec::new();
        let mut listings = 0;
        // providers/<host>/<namespace>/<type>/<version>/<os_arch>
        for host in folders(access, &["providers"], &mut listings) {
            for namespace in folders(access, &["providers", &host], &mut listings) {
                for kind in folders(access, &["providers", &host, &namespace], &mut listings) {
                    for version in folders(access, &["providers", &host, &namespace, &kind], &mut listings) {
                        let (class, source_host) = registry(&host);
                        let name = format!("{namespace}/{kind}").to_ascii_lowercase();
                        let mut item = InventoryItem::new(ItemCategory::Package, "terraform", name, rel_path)
                            .version(version.as_str())
                            .state(InstallState::Installed)
                            .source(class, source_host)
                            .detail("Providerordner vorhanden; nicht ausgeführt");
                        item.evidence.qualifier = format!("{host}/{version}");
                        items.push(item);
                    }
                }
            }
        }
        let modules = folders(access, &["modules"], &mut listings).len();
        let detail =
            format!("Arbeitsordner von init · {} Provider · {modules} Module · Inhalt nicht gelesen", items.len());
        items.insert(
            0,
            InventoryItem::new(ItemCategory::Environment, "terraform", ".terraform", rel_path).detail(detail),
        );
        bounded(items)
    }
}

/// Subfolder names, with a cap on listings for the whole inspection.
fn folders(access: &mut dyn DirAccess, path: &[&str], listings: &mut usize) -> Vec<String> {
    *listings += 1;
    if *listings > MAX_LISTINGS {
        return Vec::new();
    }
    access.list(path).into_iter().filter(|(_, is_dir)| *is_dir).map(|(name, _)| name).collect()
}

/// Registry hosts: the two public registries, anything else is private.
fn registry(host: &str) -> (SourceClass, Option<String>) {
    match valid_host(host) {
        Some(host) if PUBLIC.contains(&host.as_str()) => (SourceClass::PublicRegistry, Some(host)),
        Some(host) => (SourceClass::PrivateRegistry, Some(host)),
        None => (SourceClass::Unknown, None),
    }
}

/// `[host/]namespace/type` → (host, `namespace/type`); a bare type is
/// `hashicorp/<type>` as in Terraform 0.12.
fn provider_address(address: &str, default_host: &str) -> (String, String) {
    let address = address.trim().to_ascii_lowercase();
    match address.split('/').collect::<Vec<_>>().as_slice() {
        [host, namespace, kind] => (host.to_string(), format!("{namespace}/{kind}")),
        [namespace, kind] => (default_host.to_string(), format!("{namespace}/{kind}")),
        [kind] => (default_host.to_string(), format!("hashicorp/{kind}")),
        _ => (default_host.to_string(), address.clone()),
    }
}

/// A constraint that pins exactly one version (`1.2.3`, `= 1.2.3`).
fn exact(spec: &str) -> Option<&str> {
    let spec = spec.trim();
    let version = spec.strip_prefix('=').unwrap_or(spec).trim();
    let plain = version.starts_with(|c: char| c.is_ascii_digit())
        && version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    plain.then_some(version)
}

fn is_tofu(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".tofu")
}

fn lockfile(file: &FoundFile<'_>) -> AdapterResult {
    let mut providers = Vec::new();
    walk(file.content, |stack, frame| {
        if stack.is_empty()
            && frame.kind == Kind::Block
            && frame.name == "provider"
            && providers.len() <= crate::limits::ITEMS_PER_FILE
        {
            let attr = |key: &str| frame.attr(key).map(str::to_string);
            providers.push((frame.label.clone(), attr("version"), attr("constraints")));
        }
    })?;
    let mut items = vec![manifest("terraform", file, format!("Lockfile · {} Provider", providers.len()))];
    for (address, version, constraints) in providers {
        let Some(address) = address else { continue };
        let (host, name) = provider_address(&address, "registry.terraform.io");
        let (class, source_host) = registry(&host);
        let mut detail = String::from("aufgelöst laut .terraform.lock.hcl");
        if let Some(constraints) = constraints {
            detail.push_str(&format!(" · Spezifikation {constraints}"));
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "terraform", name, file.rel_path())
            .version(version.unwrap_or_default())
            .state(InstallState::Resolved)
            .source(class, source_host)
            .detail(detail);
        item.evidence.qualifier = host;
        items.push(item);
    }
    Ok(items)
}

/// (name or label, source, version constraint) as written.
type Requirement = (String, Option<String>, Option<String>);

fn configuration(file: &FoundFile<'_>) -> Vec<InventoryItem> {
    let tofu = is_tofu(file.name) || file.siblings.iter().any(|s| is_tofu(s));
    let default_host = if tofu { "registry.opentofu.org" } else { "registry.terraform.io" };
    let (mut required, mut resources) = (None, 0);
    let mut providers: Vec<Requirement> = Vec::new();
    let mut modules: Vec<Requirement> = Vec::new();
    let complete = walk(file.content, |stack, frame| {
        // One past the item limit is enough for `bounded` to report it.
        if providers.len() + modules.len() > crate::limits::ITEMS_PER_FILE {
            return;
        }
        let blocks: Vec<&str> =
            stack.iter().map(|f| if f.kind == Kind::Block { f.name.as_str() } else { "" }).collect();
        let attr = |key: &str| frame.attr(key).map(str::to_string);
        match (blocks.as_slice(), frame.kind, frame.name.as_str()) {
            ([], Kind::Block, "terraform") => {
                if let Some(version) = attr("required_version") {
                    required = Some(version);
                }
            }
            // Terraform 0.12: `aws = "~> 2.0"`.
            (["terraform"], Kind::Block, "required_providers") => providers.extend(
                frame.attrs.iter().filter_map(|(local, spec)| Some((local.clone(), None, Some(spec.clone()?)))),
            ),
            (["terraform", "required_providers"], Kind::Object, local) => {
                providers.push((local.to_string(), attr("source"), attr("version")))
            }
            ([], Kind::Block, "module") => {
                modules.push((frame.label.clone().unwrap_or_default(), attr("source"), attr("version")))
            }
            ([], Kind::Block, "resource" | "data") => resources += 1,
            _ => {}
        }
    })
    .is_ok();
    let kind = if is_tofu(file.name) { "OpenTofu-Konfiguration" } else { "Terraform-Konfiguration" };
    let mut detail = format!("{kind} (nicht ausgeführt)");
    if let Some(version) = &required {
        detail.push_str(&format!(" · erfordert Version {version}"));
    }
    if resources > 0 {
        detail.push_str(&format!(" · {resources} Ressourcen/Datenquellen"));
    }
    if !complete {
        detail.push_str(" · Syntax nicht vollständig gelesen");
    }
    let mut items = vec![manifest("terraform", file, detail)];
    for (local, source, spec) in providers {
        let (host, name) = provider_address(source.as_deref().unwrap_or(&local), default_host);
        let (class, source_host) = registry(&host);
        let mut detail = spec.as_deref().map_or("ohne Versionsangabe".into(), |s| format!("Spezifikation {s}"));
        if source.is_none() {
            detail.push_str(" · Quelle implizit");
        }
        if name.rsplit('/').next() != Some(local.to_ascii_lowercase().as_str()) {
            detail.push_str(&format!(" · lokaler Name {local}"));
        }
        let mut item = InventoryItem::new(ItemCategory::Package, "terraform", name, file.rel_path())
            .version(spec.as_deref().and_then(exact).unwrap_or_default())
            .state(InstallState::Declared)
            .source(class, source_host)
            .detail(detail);
        item.evidence.context = "provider".into();
        item.evidence.qualifier = local;
        items.push(item);
    }
    for (label, source, spec) in modules {
        items.push(module_item(file, &label, source.as_deref(), spec.as_deref(), default_host));
    }
    items
}

enum ModuleSource {
    /// Registry host when written, and `namespace/name/provider`.
    Registry(Option<String>, String),
    /// Git host and the `ref` query parameter.
    Git(Option<String>, Option<String>),
    Path,
    Archive(SourceClass, Option<String>),
    Other(&'static str),
}

/// Module source addresses as documented for `module` blocks.
fn module_source(source: &str) -> ModuleSource {
    let source = source.trim();
    if ["./", "../", ".\\", "..\\"].iter().any(|prefix| source.starts_with(prefix)) {
        return ModuleSource::Path;
    }
    let reference = || {
        let (_, query) = source.split_once('?')?;
        query.split('&').find_map(|pair| pair.strip_prefix("ref=")).and_then(short_ref)
    };
    if let Some((getter, rest)) = source.split_once("::") {
        return match getter {
            "git" => ModuleSource::Git(git_host(rest), reference()),
            "hg" => ModuleSource::Other("Mercurial-Quelle"),
            "s3" | "gcs" | "http" | "https" => {
                let (class, host) = classify_url(rest, PUBLIC);
                ModuleSource::Archive(class, host)
            }
            _ => ModuleSource::Other("Quelle nicht erkannt"),
        };
    }
    let lower = source.to_ascii_lowercase();
    for forge in ["github.com", "bitbucket.org"] {
        if lower.strip_prefix(forge).is_some_and(|rest| rest.starts_with('/')) {
            return ModuleSource::Git(Some(forge.to_string()), reference());
        }
    }
    if lower.starts_with("git@") {
        return ModuleSource::Git(git_host(source), reference());
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let (class, host) = classify_url(source, PUBLIC);
        return ModuleSource::Archive(class, host);
    }
    let address = source.split("//").next().unwrap_or(source);
    let segment = |s: &&str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    match address.split('/').collect::<Vec<_>>().as_slice() {
        parts @ [_, _, _] if parts.iter().all(segment) => ModuleSource::Registry(None, address.to_string()),
        [host, rest @ ..] if rest.len() == 3 && host.contains('.') && rest.iter().all(segment) => {
            match valid_host(host) {
                Some(host) => ModuleSource::Registry(Some(host), rest.join("/")),
                None => ModuleSource::Other("Quelle nicht erkannt"),
            }
        }
        _ => ModuleSource::Other("Quelle nicht erkannt"),
    }
}

fn module_item(
    file: &FoundFile<'_>,
    label: &str,
    source: Option<&str>,
    spec: Option<&str>,
    default_host: &str,
) -> InventoryItem {
    let local = |detail: String| (label.to_string(), None, detail);
    let ((name, version, detail), (class, host)) = match source.map(module_source) {
        Some(ModuleSource::Registry(host, address)) => {
            let constraint = spec.map_or("ohne Versionsangabe".into(), |s| format!("Spezifikation {s}"));
            let version = spec.and_then(exact).map(str::to_string);
            (
                (address, version, format!("Modul {label} · {constraint}")),
                registry(host.as_deref().unwrap_or(default_host)),
            )
        }
        Some(ModuleSource::Git(host, reference)) => {
            let detail =
                format!("Modul · Git-Quelle{}", reference.as_ref().map(|r| format!(" · Ref {r}")).unwrap_or_default());
            ((label.to_string(), reference, detail), (SourceClass::Git, host))
        }
        Some(ModuleSource::Path) => (local("Modul · lokaler Pfad".into()), (SourceClass::Path, None)),
        Some(ModuleSource::Archive(class, host)) => (local("Modul · Archiv-Download".into()), (class, host)),
        Some(ModuleSource::Other(kind)) => (local(format!("Modul · {kind}")), (SourceClass::Unknown, None)),
        None => (local("Modul · Quelle nicht wörtlich angegeben".into()), (SourceClass::Unknown, None)),
    };
    let mut item = InventoryItem::new(ItemCategory::Package, "terraform", name, file.rel_path())
        .version(version.unwrap_or_default())
        .state(InstallState::Declared)
        .source(class, host)
        .detail(detail);
    item.evidence.context = "module".into();
    item.evidence.qualifier = label.into();
    item
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    /// A quoted string; `None` when it holds interpolation or is a heredoc.
    Str(Option<String>),
    Open(u8),
    Close(u8),
    /// `=`, or `:` in an object constructor.
    Assign,
    Comma,
    Newline,
    Other,
}

/// Tokens of HCL native syntax. Comments are dropped, heredoc bodies and
/// template interpolations are skipped without being interpreted.
struct Lexer<'a> {
    text: &'a str,
    pos: usize,
    heredoc: Option<String>,
}

impl Lexer<'_> {
    fn skip_line(&mut self) {
        let rest = &self.text[self.pos..];
        self.pos += rest.find('\n').unwrap_or(rest.len());
    }

    /// A quoted template starting at the opening quote.
    fn string(&mut self, nesting: usize) -> Result<Option<String>, &'static str> {
        if nesting > 8 {
            return Err("Zeichenkette zu tief verschachtelt");
        }
        self.pos += 1;
        let (mut literal, mut template) = (String::new(), false);
        loop {
            let rest = &self.text[self.pos..];
            let ch = rest.chars().next().ok_or("Zeichenkette nicht abgeschlossen")?;
            match ch {
                '"' => {
                    self.pos += 1;
                    return Ok((!template).then_some(literal));
                }
                '\n' => return Err("Zeichenkette nicht abgeschlossen"),
                '\\' => {
                    let escaped = rest[1..].chars().next().ok_or("Zeichenkette nicht abgeschlossen")?;
                    literal.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                    self.pos += 1 + escaped.len_utf8();
                }
                '$' | '%' if rest[1..].starts_with(&format!("{ch}{{")) => {
                    literal.push_str(&rest[1..3]);
                    self.pos += 3;
                }
                '$' | '%' if rest[1..].starts_with('{') => {
                    template = true;
                    self.pos += 2;
                    self.interpolation(nesting)?;
                }
                _ => {
                    literal.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
            if literal.len() > 4_096 {
                template = true;
                literal.clear();
            }
        }
    }

    /// Skips `${ … }` or `%{ … }` after its opening brace.
    fn interpolation(&mut self, nesting: usize) -> Result<(), &'static str> {
        let mut depth = 1;
        loop {
            let ch = self.text[self.pos..].chars().next().ok_or("Interpolation nicht abgeschlossen")?;
            match ch {
                '"' => {
                    self.string(nesting + 1)?;
                    continue;
                }
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        return Ok(());
                    }
                }
                _ => {}
            }
            self.pos += ch.len_utf8();
        }
    }

    /// Skips heredoc lines up to the closing marker line.
    fn skip_heredoc(&mut self, marker: &str) -> Result<(), &'static str> {
        while self.pos < self.text.len() {
            let rest = &self.text[self.pos..];
            let end = rest.find('\n').map_or(rest.len(), |index| index + 1);
            self.pos += end;
            if rest[..end].trim() == marker {
                return Ok(());
            }
        }
        Err("Heredoc nicht abgeschlossen")
    }
}

impl Iterator for Lexer<'_> {
    type Item = Result<Token, &'static str>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let rest = self.text.get(self.pos..)?;
            let ch = rest.chars().next()?;
            let token = match ch {
                ' ' | '\t' | '\r' | '\u{feff}' => {
                    self.pos += ch.len_utf8();
                    continue;
                }
                '\n' => {
                    self.pos += 1;
                    if let Some(marker) = self.heredoc.take()
                        && let Err(e) = self.skip_heredoc(&marker)
                    {
                        return Some(Err(e));
                    }
                    Token::Newline
                }
                '#' => {
                    self.skip_line();
                    continue;
                }
                '/' if rest.starts_with("//") => {
                    self.skip_line();
                    continue;
                }
                '/' if rest.starts_with("/*") => {
                    match rest[2..].find("*/") {
                        Some(end) => self.pos += end + 4,
                        None => return Some(Err("Kommentar nicht abgeschlossen")),
                    }
                    continue;
                }
                '"' => return Some(self.string(0).map(Token::Str)),
                '<' if rest.starts_with("<<") => {
                    let start = if rest[2..].starts_with('-') { 3 } else { 2 };
                    let marker: String =
                        rest[start..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                    if marker.is_empty() {
                        self.pos += 2;
                        Token::Other
                    } else {
                        self.pos += start + marker.len();
                        self.heredoc = Some(marker);
                        Token::Str(None)
                    }
                }
                '{' | '[' | '(' => {
                    self.pos += 1;
                    Token::Open(ch as u8)
                }
                '}' | ']' | ')' => {
                    self.pos += 1;
                    Token::Close(ch as u8)
                }
                '=' | '!' | '<' | '>' if rest[1..].starts_with(['=', '>']) => {
                    self.pos += 2;
                    Token::Other
                }
                '=' | ':' => {
                    self.pos += 1;
                    Token::Assign
                }
                ',' => {
                    self.pos += 1;
                    Token::Comma
                }
                c if c.is_alphanumeric() || c == '_' => {
                    let len = rest.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).unwrap_or(rest.len());
                    self.pos += len;
                    Token::Word(rest[..len].to_string())
                }
                _ => {
                    self.pos += ch.len_utf8();
                    Token::Other
                }
            };
            return Some(Ok(token));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Block,
    Object,
    Other,
}

/// An open block, object or bracket while the text is walked.
struct Frame {
    kind: Kind,
    close: u8,
    name: String,
    label: Option<String>,
    /// Attributes with a literal string value (`None`: an expression).
    attrs: Vec<(String, Option<String>)>,
}

impl Frame {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.as_deref())
    }
}

/// Walks the blocks and objects of an HCL text; `visit` sees every closed
/// frame with its ancestors. Attributes are kept for the top levels only.
fn walk(text: &str, mut visit: impl FnMut(&[Frame], &Frame)) -> Result<(), String> {
    let mut lexer = Lexer { text, pos: 0, heredoc: None };
    let mut stack: Vec<Frame> = Vec::new();
    let (mut pending, mut overflow) = (Vec::new(), false);
    let mut count = 0;
    for token in lexer.by_ref() {
        count += 1;
        if count > MAX_TOKENS {
            return Err("HCL: zu viele Token".into());
        }
        match token.map_err(|e| format!("HCL: {e}"))? {
            Token::Newline | Token::Comma => statement(&mut stack, &mut pending, &mut overflow),
            Token::Open(open) => {
                let header = std::mem::take(&mut pending);
                let overflowed = std::mem::take(&mut overflow);
                let (kind, name, label) = match header.as_slice() {
                    _ if overflowed => (Kind::Other, String::new(), None),
                    [key, Token::Assign] if open == b'{' => (Kind::Object, key_text(key), None),
                    [key, Token::Assign] => {
                        if let Some(frame) = stack.last_mut().filter(|f| f.attrs.len() < 256) {
                            frame.attrs.push((key_text(key), None));
                        }
                        (Kind::Other, String::new(), None)
                    }
                    [Token::Word(kind), labels @ ..]
                        if open == b'{' && labels.iter().all(|l| matches!(l, Token::Word(_) | Token::Str(Some(_)))) =>
                    {
                        (Kind::Block, kind.clone(), labels.first().map(key_text))
                    }
                    _ => (Kind::Other, String::new(), None),
                };
                if stack.len() >= MAX_DEPTH {
                    return Err("HCL: Verschachtelung zu tief".into());
                }
                let close = match open {
                    b'{' => b'}',
                    b'[' => b']',
                    _ => b')',
                };
                stack.push(Frame { kind, close, name, label, attrs: Vec::new() });
            }
            Token::Close(close) => {
                statement(&mut stack, &mut pending, &mut overflow);
                let frame = stack.pop().ok_or("HCL: schließende Klammer ohne Gegenstück")?;
                if frame.close != close {
                    return Err("HCL: Klammern unausgeglichen".into());
                }
                visit(&stack, &frame);
            }
            other if pending.len() < 8 => pending.push(other),
            _ => overflow = true,
        }
    }
    if lexer.heredoc.is_some() {
        return Err("HCL: Heredoc nicht abgeschlossen".into());
    }
    if !stack.is_empty() {
        return Err("HCL: Block nicht geschlossen".into());
    }
    Ok(())
}

fn key_text(token: &Token) -> String {
    match token {
        Token::Word(word) => word.clone(),
        Token::Str(Some(text)) => text.clone(),
        _ => String::new(),
    }
}

/// Ends a statement: `key = "literal"` is kept on frames near the top.
fn statement(stack: &mut [Frame], pending: &mut Vec<Token>, overflow: &mut bool) {
    let tokens = std::mem::take(pending);
    let overflowed = std::mem::take(overflow);
    let depth = stack.len();
    let Some(frame) = stack.last_mut().filter(|f| depth <= 3 && f.attrs.len() < 256) else { return };
    if let [key @ (Token::Word(_) | Token::Str(Some(_))), Token::Assign, value @ ..] = tokens.as_slice()
        && !value.is_empty()
    {
        let value = match value {
            [Token::Str(Some(text))] if !overflowed => Some(text.clone()),
            _ => None,
        };
        frame.attrs.push((key_text(key), value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::testing::{FakeDir, file};

    const LOCK: &str = r#"# This file is maintained automatically by "terraform init".
# Manual edits may be lost in future updates.

provider "registry.terraform.io/hashicorp/aws" {
  version     = "5.31.0"
  constraints = "~> 5.0"
  hashes = [
    "h1:ltxyuBWIy9cq0kIKDJH1jeWJy/y7XJLjS4QrsQK4plA=",
    "zh:0cdb9c2083bf0902442384f7309367791e4640581652dda456f2d6d7abf0de8d",
  ]
}

provider "registry.opentofu.org/hashicorp/random" {
  version = "3.6.0"
  hashes = [
    "h1:5KeoKSVKVHJW308uiTgslxFbjQAdWzBGUFK68vgMRWY=",
  ]
}

provider "terraform.corp.example/platform/internal" {
  version     = "1.4.2"
  constraints = ">= 1.4.0, < 2.0.0"
}
"#;

    #[test]
    fn lockfile_lists_providers_with_hosts() {
        let items = Terraform.parse_file(&file(".terraform.lock.hcl", LOCK)).unwrap();
        assert_eq!(items[0].detail, "Lockfile · 3 Provider");
        let aws = items.iter().find(|i| i.name == "hashicorp/aws").unwrap();
        assert_eq!((aws.version.as_deref(), aws.install_state), (Some("5.31.0"), Some(InstallState::Resolved)));
        assert_eq!(
            (aws.source, aws.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("registry.terraform.io"))
        );
        assert!(aws.detail.ends_with("Spezifikation ~> 5.0"));
        let random = items.iter().find(|i| i.name == "hashicorp/random").unwrap();
        assert_eq!(random.source_host.as_deref(), Some("registry.opentofu.org"));
        let internal = items.iter().find(|i| i.name == "platform/internal").unwrap();
        assert_eq!(internal.source, SourceClass::PrivateRegistry);
        assert!(!format!("{items:?}").contains("h1:"), "hashes are not stored");
    }

    #[test]
    fn broken_lockfile_is_an_error() {
        assert!(Terraform.parse_file(&file(".terraform.lock.hcl", "provider \"a/b\" {\n  version = \"1\"\n")).is_err());
        assert!(Terraform.parse_file(&file(".terraform.lock.hcl", "provider \"a/b\" {\n version = \"1\n}\n")).is_err());
        assert!(Terraform.parse_file(&file(".terraform.lock.hcl", "}\n")).is_err());
    }

    const MAIN: &str = r#"terraform {
  required_version = ">= 1.5.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
    awsalt = { source = "hashicorp/aws", version = "5.31.0", configuration_aliases = [awsalt.west] }
    internal = {
      source  = "app.terraform.io/corp/internal"
      version = "1.2.3"
    }
    legacy = "~> 1.0"
  }

  backend "s3" {
    bucket = "state-bucket"
  }
}

locals {
  name   = "${var.prefix}-app"
  policy = <<-EOT
    {
      "Statement": [{ "Effect": "Allow" }]
      source = "evil/not/a-module"
    }
  EOT
  # module "commented" { source = "x/y/z" }
}

module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "~> 5.1"
  name    = local.name
}

module "consul" {
  source = "app.terraform.io/example-corp/consul/aws//modules/cluster"
  version = "0.11.0"
}

module "network" {
  source = "git::https://ci:hunter2@git.corp.example/infra/network.git//modules/vpc?ref=v1.2.0&depth=1"
}

module "example" {
  source = "github.com/hashicorp/example?ref=bed8e32d45c7dc98afb783b409edac98854d41a7"
}

module "local" {
  source = "./modules/local"
}

module "artifact" {
  source = "s3::https://s3-eu-west-1.amazonaws.com/corp-modules/vpc.zip"
}

resource "aws_instance" "web" {
  ami = data.aws_ami.ubuntu.id
  tags = { Name = "web-${count.index}" }
}
"#;

    #[test]
    fn configuration_declares_providers_and_modules() {
        let items = Terraform.parse_file(&file("main.tf", MAIN)).unwrap();
        assert_eq!(
            items[0].detail,
            "Terraform-Konfiguration (nicht ausgeführt) · erfordert Version >= 1.5.0 · 1 Ressourcen/Datenquellen"
        );
        let find = |name: &str, context: &str| {
            items
                .iter()
                .find(|i| i.name == name && i.evidence.context == context && i.evidence.qualifier != "awsalt")
                .unwrap_or_else(|| panic!("{name}: {items:#?}"))
        };
        let aws = find("hashicorp/aws", "provider");
        assert_eq!((aws.detail.as_str(), aws.version.as_deref()), ("Spezifikation ~> 5.0", None));
        assert_eq!(
            (aws.source, aws.source_host.as_deref()),
            (SourceClass::PublicRegistry, Some("registry.terraform.io"))
        );
        assert_eq!(aws.install_state, Some(InstallState::Declared));
        let alt = items.iter().find(|i| i.detail.contains("lokaler Name awsalt")).unwrap();
        assert_eq!(alt.version.as_deref(), Some("5.31.0"));
        let internal = find("corp/internal", "provider");
        assert_eq!(
            (internal.source, internal.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("app.terraform.io"))
        );
        let legacy = find("hashicorp/legacy", "provider");
        assert_eq!(legacy.detail, "Spezifikation ~> 1.0 · Quelle implizit");
        let vpc = find("terraform-aws-modules/vpc/aws", "module");
        assert_eq!((vpc.detail.as_str(), vpc.evidence.qualifier.as_str()), ("Modul vpc · Spezifikation ~> 5.1", "vpc"));
        assert_eq!(vpc.source, SourceClass::PublicRegistry);
        let consul = find("example-corp/consul/aws", "module");
        assert_eq!((consul.source, consul.version.as_deref()), (SourceClass::PrivateRegistry, Some("0.11.0")));
        let network = find("network", "module");
        assert_eq!((network.source, network.source_host.as_deref()), (SourceClass::Git, Some("git.corp.example")));
        assert_eq!(network.version.as_deref(), Some("v1.2.0"));
        let example = find("example", "module");
        assert_eq!(
            (example.source_host.as_deref(), example.version.as_deref()),
            (Some("github.com"), Some("bed8e32d45c7"))
        );
        assert_eq!(find("local", "module").source, SourceClass::Path);
        let artifact = find("artifact", "module");
        assert_eq!(
            (artifact.source, artifact.source_host.as_deref()),
            (SourceClass::PrivateRegistry, Some("s3-eu-west-1.amazonaws.com"))
        );
        assert!(!items.iter().any(|i| i.name.contains("evil") || i.name == "commented" || i.name == "s3"));
        assert_eq!(items.len(), 11, "{items:#?}");
        let text = format!("{items:?}");
        for secret in ["hunter2", "infra/network", "depth=1", "corp-modules", "state-bucket"] {
            assert!(!text.contains(secret), "{secret} must not be stored");
        }
    }

    #[test]
    fn opentofu_files_and_syntax_errors_stay_readable() {
        let tofu = "terraform {\n  required_providers {\n    random = { source = \"hashicorp/random\" }\n  }\n}\n";
        let items = Terraform.parse_file(&file("main.tofu", tofu)).unwrap();
        assert_eq!(items[0].detail, "OpenTofu-Konfiguration (nicht ausgeführt)");
        assert_eq!(items[1].source_host.as_deref(), Some("registry.opentofu.org"));
        let broken = "module \"a\" {\n  source = \"./a\"\n}\nresource \"x\" \"y\" {\n";
        let items = Terraform.parse_file(&file("broken.tf", broken)).unwrap();
        assert!(items[0].detail.ends_with("Syntax nicht vollständig gelesen"), "{}", items[0].detail);
        assert!(items.iter().any(|i| i.name == "a" && i.source == SourceClass::Path));
        let deep = "a = ".to_string() + &"[".repeat(200) + &"]".repeat(200);
        assert!(Terraform.parse_file(&file("deep.tf", &deep)).unwrap()[0].detail.ends_with("vollständig gelesen"));
    }

    #[test]
    fn provider_folders_beside_a_configuration() {
        assert!(Terraform.claim_dir(".terraform", &["main.tf".into()], &[]).is_some());
        assert!(Terraform.claim_dir(".terraform", &[".terraform.lock.hcl".into()], &[]).is_some());
        assert!(Terraform.claim_dir(".terraform", &["README.md".into()], &[]).is_none());
        assert!(Terraform.claim_dir("modules", &["main.tf".into()], &[]).is_none());
        assert!(Terraform.claim_dir(".terragrunt-cache", &["main.tf".into()], &[]).is_none());
        let cache = Terraform.claim_dir(".terragrunt-cache", &["terragrunt.hcl".into()], &[]).unwrap();
        let items = Terraform.inspect_dir(cache, "live/.terragrunt-cache", &mut FakeDir::default()).unwrap();
        assert_eq!((items.len(), items[0].category), (1, ItemCategory::Cache));
        let mut dir = FakeDir::default();
        let host = "providers/registry.terraform.io";
        dir.dirs.insert("providers".into(), vec![("registry.terraform.io".into(), true), ("x.txt".into(), false)]);
        dir.dirs.insert(host.into(), vec![("hashicorp".into(), true)]);
        dir.dirs.insert(format!("{host}/hashicorp"), vec![("aws".into(), true)]);
        dir.dirs.insert(format!("{host}/hashicorp/aws"), vec![("5.31.0".into(), true), ("5.30.0".into(), true)]);
        dir.dirs.insert("modules".into(), vec![("vpc".into(), true), ("modules.json".into(), false)]);
        let claim = Terraform.claim_dir(".terraform", &["main.tf".into()], &[]).unwrap();
        let items = Terraform.inspect_dir(claim, "infra/.terraform", &mut dir).unwrap();
        assert_eq!(items[0].detail, "Arbeitsordner von init · 2 Provider · 1 Module · Inhalt nicht gelesen");
        let installed: Vec<_> = items.iter().skip(1).map(|i| (i.name.as_str(), i.version.as_deref())).collect();
        assert_eq!(installed, [("hashicorp/aws", Some("5.31.0")), ("hashicorp/aws", Some("5.30.0"))]);
        assert!(items.iter().skip(1).all(|i| i.install_state == Some(InstallState::Installed)));
        assert_ne!(items[1].key(), items[2].key(), "two versions stay two items");
    }

    #[test]
    fn too_many_declarations_make_the_file_partial() {
        let text: String = (0..=crate::limits::ITEMS_PER_FILE)
            .map(|i| format!("module \"m{i}\" {{\n  source = \"./m\"\n}}\n"))
            .collect();
        assert!(Terraform.parse_file(&file("main.tf", &text)).is_err());
    }

    #[test]
    fn module_source_forms() {
        assert!(matches!(module_source("../shared"), ModuleSource::Path));
        assert!(matches!(module_source("hashicorp/consul/aws"), ModuleSource::Registry(None, _)));
        assert!(matches!(module_source("example.com/a/b"), ModuleSource::Other(_)));
        assert!(matches!(module_source("bitbucket.org/hashicorp/tf-consul"), ModuleSource::Git(Some(_), None)));
        assert!(matches!(module_source("git@github.com:hashicorp/example.git"), ModuleSource::Git(Some(_), None)));
        assert!(matches!(module_source("hg::http://example.com/vpc.hg"), ModuleSource::Other(_)));
        assert!(matches!(
            module_source("https://example.com/vpc-module.zip"),
            ModuleSource::Archive(SourceClass::PrivateRegistry, Some(_))
        ));
    }
}
