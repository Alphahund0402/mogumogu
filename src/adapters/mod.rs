//! Built-in package-manager adapters behind one capability contract
//! (PROJEKTPLAN §8.1). Adapters are *static*: they parse files handed to
//! them by the scanner and never start programs, restore, build or fetch.
//! Inventory support grants no update or cleanup capability.
mod bazel;
mod bun;
mod bundler;
mod cargo;
mod carthage;
mod cocoapods;
pub(crate) mod commands;
mod composer;
mod conan;
mod conda;
mod dart_pub;
mod deno;
mod docker;
mod elm;
mod git_submodule;
pub(crate) mod github_actions;
mod global;
mod go;
mod gradle;
mod helm;
mod julia;
mod maven;
mod mix;
mod nix;
mod npm;
mod nuget;
mod paket;
mod pdm;
mod pip;
mod pipenv;
mod pnpm;
mod poetry;
mod pre_commit;
mod renv;
mod sbt;
mod swiftpm;
mod terraform;
mod toolchain;
mod uv;
mod vcpkg;
mod yarn;
mod zig;
pub use global::inventory_global;

use crate::domain::{InventoryItem, ItemCategory, SourceClass};
use serde::Serialize;

/// Version of the adapter set; part of the comparison basis of generations.
pub const ADAPTERS_VERSION: &str = "adapters-5";

/// Support level per capability (published support matrix, §8.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Planned,
    Experimental,
    Tested,
    Limited,
    Unsupported,
}

impl Support {
    pub fn label(self) -> &'static str {
        match self {
            Self::Planned => "geplant",
            Self::Experimental => "experimentell",
            Self::Tested => "getestet",
            Self::Limited => "eingeschränkt",
            Self::Unsupported => "nicht unterstützt",
        }
    }
}

/// Where an adapter reads from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterScope {
    /// Files inside approved project folders.
    Project,
    /// A separately approved system-wide installation root or list.
    System,
}

#[derive(Clone, Debug, Serialize)]
pub struct Descriptor {
    pub id: &'static str,
    pub name: &'static str,
    /// File names (or `*.ext` patterns) parsed inside project folders.
    pub markers: &'static [&'static str],
    pub scope: AdapterScope,
    pub inventory: Support,
    pub cleanup: Support,
    /// Formats covered by fixtures, e.g. "package-lock.json v1–v3".
    pub formats: &'static [&'static str],
    pub limits: &'static str,
}

impl Descriptor {
    /// Markers are file names with at most one `*` wildcard, case-insensitive.
    pub fn wants(&self, file_name: &str) -> bool {
        let name = file_name.to_ascii_lowercase();
        self.markers.iter().any(|marker| {
            let marker = marker.to_ascii_lowercase();
            match marker.split_once('*') {
                Some((prefix, suffix)) => {
                    name.len() >= prefix.len() + suffix.len() && name.starts_with(prefix) && name.ends_with(suffix)
                }
                None => marker == name,
            }
        })
    }
}

/// A marker file handed to an adapter: bounded text, never executed.
#[derive(Clone, Copy, Debug)]
pub struct FoundFile<'a> {
    pub name: &'a str,
    pub rel_dir: &'a str,
    pub content: &'a str,
    /// Names in the same directory (for context like `.npmrc`).
    pub siblings: &'a [String],
}

impl FoundFile<'_> {
    pub fn rel_path(&self) -> String {
        crate::fsread::join(self.rel_dir, self.name)
    }
}

/// An environment/output directory that an adapter inspects shallowly
/// instead of letting the scanner descend into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirClaim {
    pub adapter: &'static str,
    pub category: ItemCategory,
    pub label: &'static str,
}

/// Bounded access below a claimed directory or system root. Paths are
/// component lists; every open is handle-relative and link-free.
pub trait DirAccess {
    /// UTF-8 text of a file, or `None` if missing, unsafe or over `max`.
    fn read_text(&mut self, path: &[&str], max: u64) -> Option<String>;
    /// Child names of a directory: `(name, is_dir)`.
    fn list(&mut self, path: &[&str]) -> Vec<(String, bool)>;
}

pub type AdapterResult = Result<Vec<InventoryItem>, String>;

pub trait Adapter: Send + Sync {
    fn descriptor(&self) -> &'static Descriptor;

    fn parse_file(&self, _file: &FoundFile<'_>) -> AdapterResult {
        Ok(Vec::new())
    }

    /// Claims a subdirectory, given its name, the names next to it and the
    /// names inside it (e.g. a venv is recognised by `pyvenv.cfg`).
    fn claim_dir(&self, _name: &str, _siblings: &[String], _children: &[String]) -> Option<DirClaim> {
        None
    }

    fn inspect_dir(&self, _claim: DirClaim, _rel_path: &str, _access: &mut dyn DirAccess) -> AdapterResult {
        Ok(Vec::new())
    }
}

/// The first adapter that claims a directory inspects it, so the node
/// managers check their own lockfile beside node_modules and Go's vendor
/// (with modules.txt) comes before Composer's and Bundler's. Runtime pins
/// claim nothing and come last.
static ALL: &[&dyn Adapter] = &[
    &npm::Npm,
    &pnpm::Pnpm,
    &bun::Bun,
    &yarn::Yarn,
    &pip::Pip,
    &uv::Uv,
    &cargo::Cargo,
    &nuget::NuGet,
    &go::Go,
    &gradle::Gradle,
    &maven::Maven,
    &poetry::Poetry,
    &pipenv::Pipenv,
    &pdm::Pdm,
    &conda::Conda,
    &paket::Paket,
    &sbt::Sbt,
    &deno::Deno,
    &composer::Composer,
    &elm::Elm,
    &bundler::Bundler,
    &dart_pub::Pub,
    &mix::Mix,
    &swiftpm::SwiftPm,
    &cocoapods::CocoaPods,
    &carthage::Carthage,
    &vcpkg::Vcpkg,
    &conan::Conan,
    &zig::Zig,
    &docker::Docker,
    &github_actions::GitHubActions,
    &pre_commit::PreCommit,
    &git_submodule::GitSubmodule,
    &terraform::Terraform,
    &helm::Helm,
    &nix::Nix,
    &bazel::Bazel,
    &renv::Renv,
    &julia::Julia,
    &toolchain::Toolchain,
];

pub fn all() -> &'static [&'static dyn Adapter] {
    ALL
}

pub fn by_id(id: &str) -> Option<&'static dyn Adapter> {
    ALL.iter().copied().find(|a| a.descriptor().id == id)
}

pub fn descriptors() -> Vec<&'static Descriptor> {
    ALL.iter().map(|a| a.descriptor()).collect()
}

/// Hint only: which adapters would look at this file name.
pub fn recognize_filename(file_name: &str) -> Vec<&'static str> {
    ALL.iter().filter(|a| a.descriptor().wants(file_name)).map(|a| a.descriptor().id).collect()
}

/// Classifies a registry/download URL. Credentials and paths are dropped.
pub(crate) fn classify_url(url: &str, public_hosts: &[&str]) -> (SourceClass, Option<String>) {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("git+") || lower.starts_with("git:") || lower.starts_with("github:") || lower.contains(".git#")
    {
        return (SourceClass::Git, None);
    }
    if lower.starts_with("file:") || lower.starts_with("link:") || lower.starts_with("workspace:") {
        return (SourceClass::Path, None);
    }
    match crate::privacy::url_host(url) {
        Some(host) if public_hosts.contains(&host.as_str()) => (SourceClass::PublicRegistry, Some(host)),
        Some(host) => (SourceClass::PrivateRegistry, Some(host)),
        None => (SourceClass::Unknown, None),
    }
}

/// Bounded number of items per file; more makes the generation partial.
pub(crate) fn bounded(items: Vec<InventoryItem>) -> AdapterResult {
    if items.len() > crate::limits::ITEMS_PER_FILE {
        return Err(format!("mehr als {} Einträge in einer Datei", crate::limits::ITEMS_PER_FILE));
    }
    if items.iter().map(|i| i.evidence.dependencies.len()).sum::<usize>() > crate::limits::ITEMS_PER_FILE {
        return Err(format!("mehr als {} Abhängigkeitsverweise in einer Datei", crate::limits::ITEMS_PER_FILE));
    }
    Ok(items)
}

pub(crate) fn manifest(adapter: &str, file: &FoundFile<'_>, detail: impl Into<String>) -> InventoryItem {
    InventoryItem::new(ItemCategory::Manifest, adapter, file.name, file.rel_path()).detail(detail)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::collections::HashMap;

    /// In-memory `DirAccess` for adapter tests.
    #[derive(Default)]
    pub struct FakeDir {
        pub files: HashMap<String, String>,
        pub dirs: HashMap<String, Vec<(String, bool)>>,
    }

    impl DirAccess for FakeDir {
        fn read_text(&mut self, path: &[&str], max: u64) -> Option<String> {
            self.files.get(&path.join("/")).filter(|t| t.len() as u64 <= max).cloned()
        }
        fn list(&mut self, path: &[&str]) -> Vec<(String, bool)> {
            self.dirs.get(&path.join("/")).cloned().unwrap_or_default()
        }
    }

    pub fn file<'a>(name: &'a str, content: &'a str) -> FoundFile<'a> {
        FoundFile { name, rel_dir: "", content, siblings: &[] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapters_without_cleanup_capability() {
        assert!(descriptors().iter().all(|d| d.cleanup == Support::Unsupported));
        // Runtime pins may read any manifest; every other dependency has
        // exactly the owners listed here.
        let owners = |name: &str| {
            recognize_filename(name).into_iter().filter(|id| *id != "toolchain").collect::<Vec<_>>().join(",")
        };
        for (names, expected) in [
            (&["uv.lock"][..], "uv"),
            (&["App.csproj", "Directory.Build.props", "Directory.Build.targets", "dotnet-tools.json"], "nuget"),
            (&["App.sln", "App.slnx", "NuGet.Config"], "nuget"),
            (
                &["requirements.txt", "requirements-dev.txt", "requirements-dev.in", "constraints.txt", "setup.cfg"],
                "pip",
            ),
            (&["pyproject.toml"], "pip,uv,poetry,pdm"),
            (&["package.json", "package-lock.json"], "npm"),
            (&["bun.lock"], "bun"),
            (&["yarn.lock"], "yarn"),
            (&["go.mod", "go.sum", "go.work", "go.work.sum"], "go"),
            (&["build.gradle.kts", "settings.gradle", "settings.gradle.kts"], "gradle"),
            (&["pom.xml", "extensions.xml"], "maven"),
            (&["poetry.lock"], "poetry"),
            (&["Pipfile", "Pipfile.lock"], "pipenv"),
            (&["pdm.lock"], "pdm"),
            (&["environment.yml", "environment.yaml", "conda-lock.yml", "prod.conda-lock.yml"], "conda"),
            (&["paket.dependencies", "paket.lock", "paket.references", "App.fsproj.paket.references"], "paket"),
            (&["build.sbt", "plugins.sbt"], "sbt"),
            (&["deno.json", "deno.jsonc", "deno.lock"], "deno"),
            (&["composer.json", "composer.lock"], "composer"),
            (&["elm.json"], "elm"),
            (&["Gemfile", "Gemfile.lock", "gems.rb", "gems.locked", "rails.gemspec"], "bundler"),
            (&["pubspec.yaml", "pubspec.lock", "pubspec_overrides.yaml"], "pub"),
            (&["mix.exs", "mix.lock"], "mix"),
            (&["Package.swift", "Package.resolved"], "swiftpm"),
            (&["Podfile", "Podfile.lock"], "cocoapods"),
            (&["Cartfile", "Cartfile.private", "Cartfile.resolved"], "carthage"),
            (&["vcpkg.json", "vcpkg-configuration.json"], "vcpkg"),
            (&["conanfile.txt", "conanfile.py", "conan.lock"], "conan"),
            (&["build.zig.zon"], "zig"),
            (&["Dockerfile", "Dockerfile.dev", "api.Dockerfile", "Containerfile", "compose.yaml"], "docker"),
            (
                &["compose.override.yml", "docker-compose.yml", "docker-compose.prod.yaml", "devcontainer.json"],
                "docker",
            ),
            (&[".devcontainer.json", ".gitlab-ci.yml", "build.gitlab-ci.yml", "bitbucket-pipelines.yml"], "docker"),
            (&["action.yml", "action.yaml"], "github-actions"),
            (&[".pre-commit-config.yaml", "prek.toml"], "pre-commit"),
            (&[".gitmodules"], "git-submodule"),
            (&[".terraform.lock.hcl", "main.tf", "main.tofu"], "terraform"),
            (&["Chart.yaml", "Chart.lock"], "helm"),
            (&["flake.nix", "flake.lock"], "nix"),
            (&["MODULE.bazel", "MODULE.bazel.lock"], "bazel"),
            (&["renv.lock"], "renv"),
            (&["Project.toml", "JuliaManifest.toml", "Manifest-v1.11.toml"], "julia"),
            (&[".nvmrc", ".tool-versions", "mise.local.toml", "rust-toolchain.toml", "global.json"], ""),
            (&["gradle-wrapper.properties"], ""),
        ] {
            for name in names {
                assert_eq!(owners(name), expected, "{name}");
            }
        }
        for pinned in [".nvmrc", "global.json", "package.json", "go.mod", "main.tf"] {
            assert!(recognize_filename(pinned).contains(&"toolchain"), "{pinned}");
        }
        for unread in ["random.txt", "terraform.tfstate", "prod.tfvars", "compose.json", "docker-compose.txt"] {
            assert!(recognize_filename(unread).is_empty(), "{unread}");
        }
        let ids: Vec<&str> = descriptors().iter().map(|d| d.id).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "adapter ids are unique");
        assert!(recognize_filename("random.txt").is_empty());
    }

    #[test]
    fn url_classification_keeps_private_hosts_private() {
        let public = &["registry.npmjs.org"];
        assert_eq!(classify_url("https://registry.npmjs.org/a/-/a-1.tgz", public).0, SourceClass::PublicRegistry);
        let (class, host) = classify_url("https://user:pw@npm.corp.example/a.tgz", public);
        assert_eq!(class, SourceClass::PrivateRegistry);
        assert_eq!(host.as_deref(), Some("npm.corp.example"));
        assert_eq!(classify_url("git+ssh://git@github.com/x/y.git", public).0, SourceClass::Git);
        assert_eq!(classify_url("file:../local", public).0, SourceClass::Path);
    }
}
