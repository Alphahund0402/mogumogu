//! Reference load (F-25; PROJEKTPLAN §12.4), scaled to a reproducible
//! fixture: 50 projects with 200 locked npm packages each (10 000 package
//! installations) plus AI files. Run explicitly in release mode:
//!
//!   cargo test --release --no-default-features --test performance -- --ignored --nocapture
//!
//! The test asserts bounded behaviour (UI pages, history, no fabricated
//! removals) and prints timings; timings are measurements, not promises.
#![cfg(windows)]

mod common;

use common::*;
use mogumogu::clock::FixedClock;
use mogumogu::config::Config;
use mogumogu::limits::UI_PAGE;
use mogumogu::service::Core;
use mogumogu::updates::NoNetwork;
use std::sync::Arc;
use std::time::Instant;

const PROJECTS: usize = 50;
const PACKAGES: usize = 200;

fn lockfile(project: usize) -> String {
    let entries: Vec<String> = (0..PACKAGES)
        .map(|i| {
            format!(
                r#""node_modules/pkg-{project}-{i}":{{"version":"1.{i}.0","resolved":"https://registry.npmjs.org/pkg-{project}-{i}/-/pkg-1.{i}.0.tgz"}}"#
            )
        })
        .collect();
    format!(r#"{{"lockfileVersion":3,"packages":{{"":{{}},{}}}}}"#, entries.join(","))
}

#[test]
#[ignore = "reference load; run explicitly in release mode"]
fn reference_inventory_stays_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::new(Some(temp.path().join("data")), false).unwrap();
    let mut core = Core::open_with(config, Arc::new(FixedClock::new(1_800_000_000)), Box::new(NoNetwork)).unwrap();
    let mut scopes = Vec::new();
    for p in 0..PROJECTS {
        let dir = temp.path().join(format!("p{p}"));
        write(&dir, "package-lock.json", &lockfile(p));
        write(&dir, "package.json", r#"{"name":"x","dependencies":{"a":"1"}}"#);
        write(&dir, "AGENTS.md", "# Regeln\n");
        write(&dir, ".claude/skills/s/SKILL.md", "---\nname: s\ndescription: d\n---\n");
        let id = core.register_project(&format!("p{p}"), &path_text(&dir)).unwrap();
        scopes.push(core.approve_project(id).unwrap());
    }

    let started = Instant::now();
    for scope in &scopes {
        let report = core.scan(*scope).unwrap();
        assert!(report.baseline && report.problem.is_none());
    }
    let first = started.elapsed();

    let started = Instant::now();
    for scope in &scopes {
        let report = core.scan(*scope).unwrap();
        assert_eq!((report.added, report.removed, report.changed), (0, 0, 0), "no event flood on rescan");
    }
    let second = started.elapsed();

    let started = Instant::now();
    let snapshot = core.snapshot().unwrap();
    let snapshot_time = started.elapsed();
    let packages: i64 = snapshot.projects.iter().filter_map(|p| p.packages).sum();
    assert_eq!(packages, (PROJECTS * PACKAGES) as i64);
    assert!(snapshot.ai.len() <= UI_PAGE as usize, "UI page bound");
    assert!(snapshot.activity.len() <= 20);

    // Same fixture, warmed database, multiple samples: a single snapshot is
    // too sensitive to background activity to judge a small improvement.
    let mut samples = Vec::with_capacity(30);
    for _ in 0..30 {
        let started = Instant::now();
        std::hint::black_box(core.snapshot().unwrap());
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    println!("Snapshot warm (30 Samples): Median {:?} · p95 {:?}", samples[15], samples[28]);

    let database = std::fs::metadata(temp.path().join("data").join("inventory.sqlite3")).unwrap().len();
    println!(
        "Referenz: {PROJECTS} Projekte, {} Paketinstallationen · Erstes Erfassen {:?} · Neuerfassung {:?} · Snapshot {:?} · DB {} KiB",
        PROJECTS * PACKAGES,
        first,
        second,
        snapshot_time,
        database / 1024
    );
}
