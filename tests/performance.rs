//! Reference load (F-25; PROJEKTPLAN §12.4), scaled to a reproducible
//! fixture: 50 projects with 200 locked npm packages each (10 000 package
//! installations) plus AI files. Run explicitly in release mode:
//!
//!   cargo test --release --no-default-features --test performance -- --ignored --nocapture
//!
//! The test asserts bounded behaviour (UI pages, history, no fabricated
//! removals) and prints timings; timings are measurements, not promises.
//! `portable_core_benchmark` runs on Linux containers and Windows alike using
//! metadata generations; the Windows-only tests also exercise live filesystem
//! scanning through the handle-relative read boundary.

#[cfg(windows)]
mod common;

#[cfg(windows)]
use common::*;
use mogumogu::clock::FixedClock;
use mogumogu::config::Config;
use mogumogu::domain::*;
use mogumogu::limits::UI_PAGE;
use mogumogu::service::Core;
use mogumogu::storage::GenerationResult;
use std::sync::Arc;
use std::time::Instant;

const PROJECTS: usize = 50;
const PACKAGES: usize = 200;

#[cfg(windows)]
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

#[cfg(windows)]
#[test]
#[ignore = "reference load; run explicitly in release mode"]
fn reference_inventory_stays_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::new(Some(temp.path().join("data")), false).unwrap();
    let clock = Arc::new(FixedClock::new(1_800_000_000));
    let mut core = Core::open_with(config, clock.clone()).unwrap();
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

    // Real refreshes happen at different seconds. Without data changes the
    // package page must stay cached until a source check turns stale.
    let mut samples = Vec::with_capacity(30);
    for _ in 0..30 {
        clock.advance(1);
        let started = Instant::now();
        std::hint::black_box(core.snapshot().unwrap());
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    println!("Snapshot mit laufender Uhr (30 Samples): Median {:?} · p95 {:?}", samples[15], samples[28]);

    // What the owner computes after changes while no dashboard is open.
    let hints = core.snapshot().unwrap().review_count();
    let mut samples = Vec::with_capacity(30);
    for _ in 0..30 {
        let started = Instant::now();
        let summary = core.tray_summary().unwrap();
        assert_eq!(summary.review_count, hints, "tray and dashboard count the same hints");
        std::hint::black_box(summary);
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    println!("Tray-Zusammenfassung (30 Samples): Median {:?} · p95 {:?}", samples[15], samples[28]);

    let mut samples = Vec::with_capacity(30);
    for _ in 0..30 {
        let started = Instant::now();
        let status = core.status().unwrap();
        assert_eq!(status.projects, PROJECTS);
        std::hint::black_box(status);
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    println!("Status (30 Samples): Median {:?} · p95 {:?}", samples[15], samples[28]);

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

/// Disk analysis of a real local folder or drive, e.g. `MOGU_DISK_BENCH=C:\`.
/// Prints counts, timings and the index's heap; no names leave the test.
#[cfg(windows)]
#[test]
#[ignore = "reads the metadata of a real folder; run explicitly in release mode"]
fn disk_analysis_of_a_real_folder() {
    use mogumogu::domain::{DiskQuery, DiskSort, DiskView};
    use mogumogu::fsread::ScopeRoot;
    use mogumogu::inventory::disk::DiskIndex;
    use mogumogu::limits::ScanLimits;
    let Some(path) = std::env::var_os("MOGU_DISK_BENCH") else {
        println!("MOGU_DISK_BENCH nicht gesetzt; nichts gemessen.");
        return;
    };
    let root = ScopeRoot::open_local(std::path::Path::new(&path), None).unwrap();
    let started = Instant::now();
    let index = DiskIndex::scan(1, "Messung", root, 0, ScanLimits::storage_analysis(), None);
    let scanned = started.elapsed();
    let s = &index.summary;
    let heap = index.memory_bytes() as f64;
    println!(
        "Analyse {scanned:?} · {} Einträge ({} Dateien, {} Ordner, {} ausgelassen, {} nicht lesbar) · vollständig {} {:?}",
        s.entries, s.files, s.folders, s.skipped, s.unreadable, s.complete, s.reason
    );
    println!(
        "Verzeichnis im Speicher: {:.1} MiB · {:.1} Bytes je Eintrag",
        heap / 1_048_576.0,
        heap / (s.files + s.folders).max(1) as f64
    );
    for (label, query) in [
        ("Ordner", DiskQuery::default()),
        ("Größte Dateien", DiskQuery { view: DiskView::Files, ..Default::default() }),
        ("Dateitypen", DiskQuery { view: DiskView::Types, ..Default::default() }),
        ("Duplikat-Kandidaten", DiskQuery { view: DiskView::Duplicates, ..Default::default() }),
        ("Filter *.dll", DiskQuery { view: DiskView::Files, text: "*.dll".into(), ..Default::default() }),
        ("Nach Name", DiskQuery { view: DiskView::Files, sort: DiskSort::Name, ..Default::default() }),
    ] {
        let started = Instant::now();
        let page = index.page(&query).unwrap();
        println!("{label}: {} von {} in {:?}", page.rows.len(), page.total, started.elapsed());
    }
}

/// Portable core benchmark that runs in isolated Linux Docker containers and
/// on Windows without touching host filesystems. Exercises SQLite generation
/// publishing, snapshots, package paging/filtering/retention, tidy history
/// writes, and adapter/AI filename prefilters.
#[test]
#[ignore = "portable reference load; run explicitly in release mode"]
fn portable_core_benchmark() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::new(Some(temp.path().join("data")), false).unwrap();
    let clock = Arc::new(FixedClock::new(1_800_000_000));
    let mut core = Core::open_with(config, clock.clone()).unwrap();

    // 50 registered projects + 1 drive-search scope with 2 000 discoveries
    // to exercise the realistic mix of packages, AI artifacts, outputs and discoveries.
    let mut project_scopes = Vec::with_capacity(PROJECTS);
    let mut project_ids = Vec::with_capacity(PROJECTS);
    for p in 0..PROJECTS {
        let path = format!(r"C:\fixture\project-{p}");
        let pid = core.register_project(&format!("p{p}"), &path).unwrap();
        let sid = core.repository().approve_scope(ScopeKind::Project, Some(pid), &path, None).unwrap();
        project_ids.push(pid);
        project_scopes.push(sid);
    }
    let search_scope = core.repository().approve_scope(ScopeKind::ProjectSearch, None, r"C:\fixture", None).unwrap();

    let project_items = |p: usize| -> GenerationResult {
        let mut items = Vec::with_capacity(PACKAGES + 5);
        items.push(InventoryItem::new(ItemCategory::Manifest, "npm", "package.json", "package.json"));
        items.push(InventoryItem::new(ItemCategory::AiArtifact, "codex", "Instruktionen: AGENTS.md", "AGENTS.md"));
        items.push(InventoryItem::new(
            ItemCategory::AiArtifact,
            "claude-code",
            "Skill: review",
            ".claude/skills/review/SKILL.md",
        ));
        items.push(InventoryItem::new(ItemCategory::Output, "npm", "dist", "dist"));
        items.push(InventoryItem::new(ItemCategory::Environment, "npm", "node_modules", "node_modules"));
        for i in 0..PACKAGES {
            items.push(
                InventoryItem::new(ItemCategory::Package, "npm", format!("pkg-{p}-{i}"), "package-lock.json")
                    .version(format!("1.{i}.0"))
                    .state(InstallState::Resolved)
                    .source(SourceClass::PublicRegistry, Some("registry.npmjs.org".into())),
            );
        }
        GenerationResult { items, entries_seen: 250, files_read: 4, bytes_read: 40_000, ..Default::default() }
    };

    let discovery_items = || -> GenerationResult {
        let mut items = Vec::with_capacity(2_000);
        for i in 0..2_000 {
            let mut item = InventoryItem::new(
                ItemCategory::Environment,
                "discovery",
                format!("cand-{i}"),
                format!("work/cand-{i}"),
            );
            item.evidence.discovery = Some(DiscoveryEvidence {
                kind: if i % 4 == 0 { DiscoveryKind::Project } else { DiscoveryKind::Scratchpad },
                is_dir: true,
                modified_at: Some(1_799_000_000),
                has_project_context: true,
                path_exact: true,
                confidence: 3,
                signals: vec!["Git".into()],
            });
            items.push(item);
        }
        GenerationResult { items, entries_seen: 20_000, ..Default::default() }
    };

    let started = Instant::now();
    for (p, &scope) in project_scopes.iter().enumerate() {
        let generation = core.repository().begin_generation(scope, "bench-v1").unwrap();
        let report = core.repository().finish_generation(generation, &project_items(p)).unwrap();
        assert!(report.baseline);
    }
    let generation = core.repository().begin_generation(search_scope, "bench-v1").unwrap();
    core.repository().finish_generation(generation, &discovery_items()).unwrap();
    let publish_first = started.elapsed();

    let started = Instant::now();
    for (p, &scope) in project_scopes.iter().enumerate() {
        let generation = core.repository().begin_generation(scope, "bench-v1").unwrap();
        let report = core.repository().finish_generation(generation, &project_items(p)).unwrap();
        assert_eq!((report.added, report.removed, report.changed), (0, 0, 0));
    }
    let publish_rescan = started.elapsed();

    let started = Instant::now();
    let snapshot = core.snapshot().unwrap();
    let snapshot_cold = started.elapsed();
    assert_eq!(snapshot.projects.iter().filter_map(|p| p.packages).sum::<i64>(), (PROJECTS * PACKAGES) as i64);
    assert!(snapshot.ai.len() <= UI_PAGE as usize);
    assert!(snapshot.discoveries.len() <= UI_PAGE as usize);

    let mut warm_samples = Vec::with_capacity(30);
    for _ in 0..30 {
        let started = Instant::now();
        std::hint::black_box(core.snapshot().unwrap());
        warm_samples.push(started.elapsed());
    }
    warm_samples.sort_unstable();

    let mut clock_samples = Vec::with_capacity(30);
    for _ in 0..30 {
        clock.advance(1);
        let started = Instant::now();
        std::hint::black_box(core.snapshot().unwrap());
        clock_samples.push(started.elapsed());
    }
    clock_samples.sort_unstable();

    // Cold package query (filter/search/page miss) + retain_package check
    let started = Instant::now();
    let filtered = core
        .package_page(
            &PackageQuery { text: "pkg-12".into(), filter: PackageFilter::All, ecosystem: "npm".into() },
            0,
            60,
        )
        .unwrap();
    let pkg_filter_time = started.elapsed();
    assert_eq!(filtered.counts.all, PACKAGES);

    let first_key = filtered.packages[0].key.clone();
    let started = Instant::now();
    core.retain_package(&first_key, true).unwrap();
    let retain_cached_time = started.elapsed();

    // Non-cached key forces DB fallback `has_published_package_key`
    let page2 = core.package_page(&PackageQuery::default(), 60, 60).unwrap();
    let other_key = page2.packages[0].key.clone();
    let started = Instant::now();
    let _ = core.package_page(&PackageQuery::default(), 0, 60).unwrap();
    core.retain_package(&other_key, true).unwrap();
    let retain_db_time = started.elapsed();

    // Combined tidy history + project size write across all 50 projects
    let started = Instant::now();
    for &pid in &project_ids {
        let point = TidyPoint {
            checked_at: 1_800_000_000,
            complete: true,
            total: Some(1_000_000),
            regenerable: 500_000,
            review: 100_000,
            vcs: 50_000,
            test_data: 0,
            agent: 0,
            items: vec![("target".into(), TidyClass::Regenerable, 500_000)],
        };
        core.repository().record_tidy_and_size(pid, Some(1_000_000), &point).unwrap();
    }
    let tidy_write_time = started.elapsed();

    let mut tray_samples = Vec::with_capacity(30);
    for _ in 0..30 {
        let started = Instant::now();
        std::hint::black_box(core.tray_summary().unwrap());
        tray_samples.push(started.elapsed());
    }
    tray_samples.sort_unstable();

    let mut status_samples = Vec::with_capacity(30);
    for _ in 0..30 {
        let started = Instant::now();
        let status = core.status().unwrap();
        assert_eq!(status.projects, PROJECTS);
        std::hint::black_box(status);
        status_samples.push(started.elapsed());
    }
    status_samples.sort_unstable();

    // Prefilter benchmark over 50 000 file names
    let names = ["main.rs", "index.tsx", "utils.py", "README.md", "package.json", "SKILL.md", "App.csproj", "data.bin"];
    let catalog = mogumogu::profiles::catalog();
    let started = Instant::now();
    let mut hits = 0usize;
    for i in 0..50_000 {
        let name = names[i % names.len()];
        hits += mogumogu::adapters::recognize_filename(name).len();
        hits += usize::from(catalog.may_match(name));
    }
    std::hint::black_box(hits);
    let prefilter_time = started.elapsed();

    let database = std::fs::metadata(temp.path().join("data").join("inventory.sqlite3")).unwrap().len();
    println!("Portable Benchmark (50 Projekte, 10000 Pakete, 2000 Funde · DB {} KiB):", database / 1024);
    println!("  Publish Erstlauf / Neuerfassung: {publish_first:?} / {publish_rescan:?}");
    println!("  Snapshot kalt: {snapshot_cold:?}");
    println!("  Snapshot warm (30 Samples): Median {:?} · p95 {:?}", warm_samples[15], warm_samples[28]);
    println!("  Snapshot laufende Uhr (30 Samples): Median {:?} · p95 {:?}", clock_samples[15], clock_samples[28]);
    println!(
        "  Paket-Filter (pkg-12): {pkg_filter_time:?} · retain (Cache / DB): {retain_cached_time:?} / {retain_db_time:?}"
    );
    println!("  Tidy-Verlauf + Projektgröße (50 Projekte): {tidy_write_time:?}");
    println!("  Tray-Zusammenfassung (30 Samples): Median {:?} · p95 {:?}", tray_samples[15], tray_samples[28]);
    println!("  Status (30 Samples): Median {:?} · p95 {:?}", status_samples[15], status_samples[28]);
    println!("  Adapter-/KI-Prefilter (50000 Dateinamen): {prefilter_time:?}");
}
