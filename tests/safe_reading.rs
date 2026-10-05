//! Windows read-boundary tests (F-06, AT-05, AT-06, AT-07, AT-08).
#![cfg(windows)]

mod common;

use common::*;
use mogumogu::domain::{InstallState, ItemCategory, ScopeKind};
use mogumogu::fsread::{RootError, ScopeRoot};
use mogumogu::inventory::{self, ScanFailure, ScanRequest};
use mogumogu::limits::ScanLimits;
use mogumogu::platform;
use std::fs;

fn request(path: &std::path::Path) -> ScanRequest {
    ScanRequest {
        kind: ScopeKind::Project,
        path: path.to_path_buf(),
        identity: Some(platform::path_identity(path).unwrap()),
        limits: ScanLimits::default(),
    }
}

#[test]
fn project_scan_finds_states_without_executing_anything() {
    let temp = tempfile::tempdir().unwrap();
    let project = sample_project(temp.path());
    let outcome = inventory::scan(&request(&project));
    let result = outcome.result;
    assert!(outcome.failure.is_none());
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let has = |category, name: &str, state| {
        result.items.iter().any(|i| i.category == category && i.name == name && i.install_state == state)
    };
    assert!(has(ItemCategory::Package, "left-pad", Some(InstallState::Declared)));
    assert!(has(ItemCategory::Package, "left-pad", Some(InstallState::Resolved)));
    assert!(has(ItemCategory::Package, "left-pad", Some(InstallState::Installed)));
    assert!(has(ItemCategory::Package, "requests", Some(InstallState::Installed)));
    assert!(has(ItemCategory::Output, "target", None));
    assert!(result.items.iter().any(|i| i.category == ItemCategory::AiArtifact && i.name == "Skill: review"));
    assert!(
        result.items.iter().any(|i| i.category == ItemCategory::AiReference
            && i.name == "guide.md"
            && i.detail.starts_with("vorhanden"))
    );
    let dump = format!("{:?}", result.items);
    for forbidden in ["ghp_", "someone", "Lösche", "calc", "evil.js"] {
        assert!(!dump.contains(forbidden), "{forbidden} must not be stored");
    }
    // node_modules is inspected shallowly, never descended into.
    assert!(!result.items.iter().any(|i| i.rel_path.contains("left-pad/index.js")));
}

#[test]
fn junctions_are_not_followed() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let outside = temp.path().join("outside");
    write(&outside, "package.json", r#"{"dependencies":{"secret-outside":"1"}}"#);
    write(&outside, "AGENTS.md", "# draußen");
    write(&project, "package.json", r#"{"dependencies":{"inside":"1"}}"#);
    junction(&project.join("linked"), &outside);
    let outcome = inventory::scan(&request(&project));
    let names: Vec<&str> = outcome.result.items.iter().map(|i| i.name.as_str()).collect();
    assert!(names.contains(&"inside"));
    assert!(!names.contains(&"secret-outside"), "junction target was read: {names:?}");
    assert!(outcome.result.notes.iter().any(|n| n.contains("Verknüpfungen")));
}

#[test]
fn replaced_root_does_not_inherit_the_approval() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    write(&project, "package.json", "{}");
    let approved = platform::path_identity(&project).unwrap();
    fs::remove_dir_all(&project).unwrap();
    write(&project, "package.json", "{}");
    assert!(matches!(ScopeRoot::open(&project, Some(approved)), Err(RootError::IdentityChanged)));
    let outcome = inventory::scan(&ScanRequest { identity: Some(approved), ..request(&project) });
    assert_eq!(outcome.failure, Some(ScanFailure::IdentityChanged));
    assert!(outcome.result.items.is_empty());
}

#[test]
fn a_junction_as_root_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    let link = temp.path().join("link");
    junction(&link, &target);
    assert!(platform::path_identity(&link).is_err());
    assert!(ScopeRoot::open(&link, None).is_err());
}

#[test]
fn missing_root_is_unavailable_not_empty() {
    let temp = tempfile::tempdir().unwrap();
    let outcome = inventory::scan(&ScanRequest {
        kind: ScopeKind::Project,
        path: temp.path().join("missing"),
        identity: None,
        limits: ScanLimits::default(),
    });
    assert!(matches!(outcome.failure, Some(ScanFailure::Unavailable(_))));
    assert!(outcome.result.failed);
}

#[test]
fn limits_make_a_pass_partial() {
    let temp = tempfile::tempdir().unwrap();
    let project = sample_project(temp.path());
    let limits = ScanLimits { max_entries: 3, ..ScanLimits::default() };
    let outcome = inventory::scan(&ScanRequest { limits, ..request(&project) });
    assert!(outcome.result.limit_reason.is_some());
    assert_eq!(outcome.result.state(), mogumogu::domain::GenerationState::Partial);
}

#[test]
fn oversized_files_are_reported_not_read() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("p");
    write(&project, "AGENTS.md", &"x".repeat(2 * 1024 * 1024));
    let outcome = inventory::scan(&request(&project));
    assert!(outcome.result.errors.iter().any(|e| e.contains("zu groß")), "{:?}", outcome.result.errors);
}

#[test]
fn measurement_is_complete_or_unknown() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("m");
    write(&dir, "a.bin", &"a".repeat(1000));
    write(&dir, "sub/b.bin", &"b".repeat(500));
    let measured = inventory::measure(&dir, None);
    assert_eq!(measured.bytes, Some(1500));
    assert_eq!(inventory::measure(&temp.path().join("nope"), None).bytes, None);
}
