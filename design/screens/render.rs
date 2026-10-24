//! Native screen renders for review and documentation. Uses the compiled
//! `Dashboard` and the real `src/desktop/view.rs` mapping, fed with a
//! synthetic snapshot: no database, no filesystem scan, no user data.
//! This is an offscreen software render, not a Windows/DWM screenshot.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};
use std::{path::Path, rc::Rc};

mod desktop {
    #[allow(dead_code, unused_imports, clippy::all)]
    pub mod ui {
        include!(env!("MOGU_UI_MODULE"));
    }
    #[path = "../../../src/desktop/view.rs"]
    #[allow(dead_code)]
    pub mod view;
    #[path = "../../../src/desktop/disk.rs"]
    #[allow(dead_code)]
    pub mod disk;
    #[path = "../../../src/desktop/dialogs.rs"]
    #[allow(dead_code)]
    pub mod dialogs;

    /// The drive view shares `Desktop` with the app; rendering never submits.
    pub struct Owner;
    impl Owner {
        pub fn submit(&self, _: mogumogu::service::Request) -> bool {
            false
        }
    }
    pub struct Desktop {
        pub folder_pending: Option<(i64, String)>,
        pub owner: Owner,
        pub status: String,
        pub dashboard: Option<ui::Dashboard>,
    }
    impl Desktop {
        pub fn render_status(&self) {}
    }
}

use desktop::ui::*;
use desktop::{dialogs, view};
use mogumogu::clock::{Clock, DAY, HOUR, MINUTE, SystemClock};
use mogumogu::domain::*;
use mogumogu::service::ScanProgress;

struct RenderPlatform(Rc<MinimalSoftwareWindow>);
impl Platform for RenderPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn write_ppm(path: &Path, pixels: &[Rgb8Pixel], width: usize, height: usize) {
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    bytes.extend(pixels.iter().flat_map(|p| [p.r, p.g, p.b]));
    std::fs::write(path, bytes).unwrap();
}

fn generation(id: i64, state: GenerationState, finished: i64, items: i64, limit: Option<&str>) -> GenerationSummary {
    GenerationSummary {
        id,
        state,
        started_at: finished - 20,
        finished_at: Some(finished),
        entries_seen: items * 9,
        files_read: items / 3,
        items,
        limit_reason: limit.map(str::to_string),
        error_count: 0,
    }
}

fn scope(id: i64, project: Option<i64>, kind: ScopeKind, path: &str, published: GenerationSummary, latest: Option<GenerationSummary>) -> Scope {
    Scope {
        id,
        project_id: project,
        kind,
        path: path.into(),
        status: ScopeStatus::Approved,
        approved_at: 0,
        last_error: None,
        latest: Some(latest.unwrap_or_else(|| published.clone())),
        published: Some(published),
    }
}

fn project(id: i64, name: &str, path: &str, eco: &str, state: Observation, bytes: i64, packages: Option<i64>) -> Project {
    Project {
        id,
        name: name.into(),
        path: path.into(),
        ecosystem: eco.into(),
        state,
        evidence: "Synthetische Renderdaten".into(),
        bytes: Some(bytes),
        packages,
        protected: true,
        origin: Origin::Registered,
        read_approved: true,
        coverage: String::new(),
    }
}

fn resource(id: i64, project: i64, name: &str, path: &str, kind: ResourceKind, parent: Option<i64>, now: i64) -> Resource {
    Resource {
        id,
        project_id: Some(project),
        name: name.into(),
        path: path.into(),
        kind,
        bytes: None,
        state: Observation::Unknown,
        evidence: "Von mogumogu angelegt".into(),
        protected: true,
        origin: Origin::Registered,
        acquisition: Acquisition::Managed,
        parent_id: parent,
        purpose: None,
        review_at: None,
        created_at: Some(now - 12 * DAY),
    }
}

fn recycle_plan(now: i64) -> PlanSummary {
    let locked = "Gesperrt bis zum unabhängigen Review (G5): vorher wird nur in einer registrierten wegwerfbaren Testwurzel in den Papierkorb verschoben.";
    let target = |id: i64, rel_path: &str, label: &str, evidence: &str, bytes: i64| PlanTarget {
        id,
        rel_path: rel_path.into(),
        label: label.into(),
        evidence: evidence.into(),
        bytes,
        blockers: vec![locked.into()],
        state: JournalState::Planned,
        error: None,
    };
    PlanSummary {
        id: 3,
        state: PlanState::Draft,
        created_at: now,
        fingerprint: "a3f09c2e71b4d8e65f10".into(),
        project_id: Some(1),
        project: "my-app".into(),
        root: r"C:\dev\my-app".into(),
        targets: vec![
            target(1, "node_modules", "Abhängigkeiten (node_modules)", "package-lock.json daneben", 1_240_000_000),
            target(2, ".next", "Build-Ausgabe", "package.json daneben", 312_000_000),
        ],
        kept: vec![
            "Quellen, Projekt- und Lockfiles".into(),
            "Versionsverwaltung (.git)".into(),
            "Alles unter „Prüfen“: Testdaten, Datenbanken, Logs, große Dateien".into(),
            "Jeder Ordner, der nicht im Plan steht".into(),
        ],
        recovery: "Die Ordner landen im Windows-Papierkorb und lassen sich dort wiederherstellen. Neu erzeugen lassen sie sich mit dem Werkzeug aus dem Beleg (etwa npm ci, cargo build oder uv sync). Der Speicher wird erst frei, wenn du den Papierkorb leerst.".into(),
        done: 0,
        failed: 0,
        note: None,
    }
}

fn finding(id: &str, group: FindingGroup, name: &str, location: &str, evidence: FindingEvidence, note: &str, scope: Option<ScopeKind>, scope_id: Option<i64>) -> SystemFinding {
    SystemFinding {
        id: id.into(),
        group,
        name: name.into(),
        location: location.into(),
        path: location.into(),
        evidence,
        note: note.into(),
        scope,
        scope_id,
        coverage: if scope_id.is_some() { "Vollständig erfasst · 2 Einträge".into() } else { String::new() },
    }
}

#[allow(clippy::too_many_arguments)]
fn package(name: &str, version: &str, eco: &str, host: &str, project: &str, deprecated: bool) -> PackageFinding {
    let id = PackageId {
        ecosystem: eco.into(),
        name: name.into(),
        version: version.into(),
        source: SourceClass::PublicRegistry,
        source_host: host.into(),
    };
    PackageFinding {
        key: format!("{eco}:{name}:{version}"),
        id,
        occurrences: if project.is_empty() {
            Vec::new()
        } else {
            vec![PackageOccurrence {
                scope_id: 1,
                project_id: Some(1),
                project: project.into(),
                rel_path: "package-lock.json".into(),
                state: Some(InstallState::Resolved),
                detail: String::new(),
                current: true,
                owned_files: None,
            }]
        },
        references: if project.is_empty() { Vec::new() } else { vec![format!("{project} · package-lock.json · aufgelöst")] },
        possible_references: Vec::new(),
        local_deprecation: deprecated.then(|| "Version laut Lockfile abgekündigt".into()),
        retained: false,
        coverage: "4 Projekte vollständig".into(),
        next_action: if deprecated {
            "Projektbelege und Abhängigkeitskette prüfen; bei transitiver Abhängigkeit das übergeordnete Paket aktualisieren. Änderungen und Tests separat ausführen.".into()
        } else {
            "In Projektmetadaten referenziert. Deklaration und aufgelöste Version vor Änderungen prüfen.".into()
        },
        review: deprecated,
    }
}

fn ai(client: &str, kind: &str, label: &str, path: &str, scope: &str) -> AiEntry {
    AiEntry {
        client: client.into(),
        label: label.into(),
        path: path.into(),
        state: "protected".into(),
        kind: kind.into(),
        scope: scope.into(),
        references: Vec::new(),
        scope_id: None,
        rel_path: String::new(),
        viewable: true,
    }
}

fn hint(id: i64, name: &str, kind: HintKind, trigger: &str, evidence: &str) -> ReviewHint {
    ReviewHint {
        resource_id: id,
        resource_name: name.into(),
        kind,
        trigger: trigger.into(),
        evidence: evidence.into(),
        observed_period: "12 Tage Beobachtung".into(),
        owner: "ml-experiments (ausdrücklich)".into(),
        uncertainty: "Keine beobachtete Nutzung ist kein Entbehrlichkeitsnachweis.".into(),
        next_action: "Verlängern, schützen oder zum Projekt machen.".into(),
        protected: true,
        blockers: Vec::new(),
        bytes: None,
    }
}

/// A local inventory after a few weeks of use. Every value is invented.
fn fixture(now: i64) -> Snapshot {
    let complete = |id, items| generation(id, GenerationState::Complete, now - 25 * MINUTE, items, None);
    let scopes = vec![
        scope(1, Some(1), ScopeKind::Project, r"C:\dev\my-app", complete(11, 412), None),
        scope(2, Some(2), ScopeKind::Project, r"D:\code\website", complete(12, 230), None),
        scope(3, Some(3), ScopeKind::Project, r"C:\dev\ml", complete(13, 318), None),
        scope(4, Some(4), ScopeKind::Project, r"C:\tools\dev", complete(14, 61), None),
        scope(
            5,
            Some(5),
            ScopeKind::Project,
            r"D:\projects\game",
            complete(15, 140),
            Some(generation(16, GenerationState::Partial, now - 25 * MINUTE, 90, Some("Eintragslimit erreicht"))),
        ),
        scope(6, None, ScopeKind::NpmGlobal, r"C:\Users\x\AppData\Roaming\npm\node_modules", complete(17, 2), None),
        scope(7, None, ScopeKind::UserAi, r"C:\Users\x\.claude", complete(18, 6), None),
    ];
    let projects = vec![
        project(1, "my-app", r"C:\dev\my-app", "npm · TypeScript", Observation::Observed, 12_400_000_000, Some(160)),
        project(2, "website", r"D:\code\website", "pnpm · TypeScript", Observation::Observed, 856_000_000, Some(88)),
        project(3, "ml-experiments", r"C:\dev\ml", "uv · Python", Observation::Review, 25_400_000_000, Some(112)),
        project(4, "tools", r"C:\tools\dev", "Cargo · Rust", Observation::Observed, 342_000_000, Some(18)),
        project(5, "game-prototype", r"D:\projects\game", "NuGet · C#", Observation::Unknown, 7_600_000_000, Some(38)),
        project(6, "old-stuff", r"D:\archive\experiments", "npm · JavaScript", Observation::Unknown, 5_400_000_000, None),
    ];
    let mut spike = resource(20, 3, "parser-spike", r"C:\dev\scratch\parser-spike", ResourceKind::Scratchpad, None, now);
    spike.purpose = Some("Tokenizer testen".into());
    spike.review_at = Some(now - 2 * DAY);
    let mut lab = resource(21, 1, "api-lab", r"C:\dev\scratch\api-lab", ResourceKind::Scratchpad, None, now);
    lab.purpose = Some("API-Prototyp".into());
    lab.review_at = Some(now + 5 * DAY);
    let mut bench = resource(22, 4, "render-bench", r"C:\dev\scratch\render-bench", ResourceKind::Scratchpad, None, now);
    bench.purpose = Some("Benchmark".into());
    bench.review_at = Some(now + 6 * DAY);
    let mut smoke = resource(23, 1, "smoke-test", r"C:\dev\scratch\smoke-test", ResourceKind::Scratchpad, None, now);
    smoke.purpose = Some("Rauchtest".into());
    smoke.review_at = Some(now + 3 * DAY);
    let mut source = resource(24, 3, "parser-spike · source", r"C:\dev\scratch\parser-spike\source", ResourceKind::Source, Some(20), now);
    source.bytes = Some(2_100_000);
    let mut results = resource(25, 3, "parser-spike · results", r"C:\dev\scratch\parser-spike\results", ResourceKind::Results, Some(20), now);
    results.bytes = Some(48_000_000);
    let mut environment = resource(26, 3, "parser-spike · environment", r"C:\dev\scratch\parser-spike\environment", ResourceKind::Environment, Some(20), now);
    environment.bytes = Some(1_100_000_000);
    let mut temporary = resource(27, 3, "parser-spike · temporary", r"C:\dev\scratch\parser-spike\temporary", ResourceKind::Temporary, Some(20), now);
    temporary.bytes = Some(212_000_000);
    temporary.protected = false;
    let resources = vec![spike, lab, bench, smoke, source, results, environment, temporary];
    let hints = vec![
        hint(20, "parser-spike", HintKind::ReviewDue, "Prüftermin seit 2 Tagen erreicht.", "Scratchpad-Regel (7 Tage)"),
        hint(21, "api-lab", HintKind::PossiblyOrphaned, "Kein ausdrücklicher oder vermuteter Besitzer bekannt.", "Registriert ohne Projektzuordnung."),
        hint(26, "parser-spike · environment", HintKind::PossiblyOrphaned, "Kein ausdrücklicher oder vermuteter Besitzer bekannt.", "Registriert ohne Projektzuordnung."),
    ];
    let mut packages = vec![
        package("left-pad", "1.3.0", "npm", "registry.npmjs.org", "my-app", true),
        package("request", "2.88.2", "npm", "registry.npmjs.org", "website", true),
        package("numpy", "1.26.4", "python", "pypi.org", "ml-experiments", false),
        package("serde", "1.0.228", "cargo", "crates.io", "tools", false),
        package("Newtonsoft.Json", "13.0.3", "nuget", "api.nuget.org", "game-prototype", false),
        package("typescript", "5.4.5", "npm", "registry.npmjs.org", "", false),
        package("black", "24.4.2", "python", "pypi.org", "", false),
    ];
    packages[5].possible_references = vec!["Befehlsname tsc in package.json-Skripten".into()];
    let report = PackageReport {
        coverage: PackageCoverage { complete_projects: 4, incomplete_projects: 2, discovery_roots: 0, incomplete_roots: 0, truncated: false },
        packages,
        total: 85,
        truncated: true,
        offset: 0,
        query: PackageQuery::default(),
        counts: PackageCounts { all: 85, review: 7, deprecated: 2, unreferenced: 12, kept: 2 },
    };
    use FindingEvidence::*;
    use FindingGroup::*;
    let system = SystemReport {
        checked_at: Some(now - 40 * MINUTE),
        path_entries: 45,
        locations: 21,
        findings: vec![
            finding("tool:git", Tool, "Git", r"%ProgramFiles%\Git\mingw64\bin\git.exe", OnPath, "Im Suchpfad gefunden · nicht gestartet", None, None),
            finding("tool:node", Tool, "Node.js", r"C:\nvm4w\nodejs\node.exe", Shim, "Suchpfad-Ordner ist eine Verknüpfung (Versionsumschalter) · nicht gestartet", None, None),
            finding("tool:npm", Tool, "npm", r"C:\nvm4w\nodejs\npm.cmd", Shim, "Suchpfad-Ordner ist eine Verknüpfung (Versionsumschalter) · nicht gestartet", None, None),
            finding("tool:python", Tool, "python", r"%USERPROFILE%\.pyenv\pyenv-win\shims\python.bat", Shim, "pyenv-Shim · keine globale Version gewählt · nicht gestartet", None, None),
            finding("tool:uv", Tool, "uv · uvx", r"%USERPROFILE%\.local\bin\uv.exe", OnPath, "Im Suchpfad gefunden · nicht gestartet", None, None),
            finding("tool:cargo", Tool, "Cargo", r"%USERPROFILE%\.cargo\bin\cargo.exe", Shim, "rustup-Proxy oder Cargo-Tool · nicht gestartet", None, None),
            finding("tool:dotnet", Tool, ".NET (dotnet)", r"%ProgramFiles%\dotnet\dotnet.exe", OnPath, "Im Suchpfad gefunden · nicht gestartet", None, None),
            finding("tool:winget", Tool, "WinGet", r"%LOCALAPPDATA%\Microsoft\WindowsApps\winget.exe", Alias, "Verknüpfung oder App-Alias, nicht verfolgt · nicht gestartet", None, None),
            finding("tool:claude", Tool, "Claude Code", r"%USERPROFILE%\.local\bin\claude.exe", OnPath, "Im Suchpfad gefunden · nicht gestartet", None, None),
            finding("store:npm-global:0", Store, "Globale npm-Pakete", r"%APPDATA%\npm\node_modules", Location, "Paketordner", Some(ScopeKind::NpmGlobal), Some(6)),
            finding("store:uv-tools:1", Store, "uv-Toolumgebungen", r"%APPDATA%\uv\tools", Location, "Ort weicht vom aktuellen Standard ab · Toolumgebungen mit uv-receipt.toml", Some(ScopeKind::UvTools), None),
            finding("store:cargo-home:0", Store, "Cargo-Installationen", r"%USERPROFILE%\.cargo", Installed, "Installationsdatensätze; bin enthält auch rustup-Proxys", Some(ScopeKind::CargoTools), None),
            finding("store:nuget-cache:0", Store, "NuGet-Paketordner", r"%USERPROFILE%\.nuget\packages", Location, "Gemeinsamer Speicher, keine Projektzuordnung", Some(ScopeKind::NugetCache), None),
            finding("store:pyenv:0", Store, "pyenv-win", r"%USERPROFILE%\.pyenv\pyenv-win", Location, "Interpreterauswahl ungeprüft; nichts wird gestartet", None, None),
            finding("store:claude-temp:0", Scratch, "Arbeitsordner von Claude Code", r"%USERPROFILE%\AppData\Local\Temp\claude", Location, "Scratchpads und Ausgaben von KI-Sitzungen; nur Namen werden gelesen", Some(ScopeKind::ProjectSearch), None),
            finding("store:jetbrains-scratches:0", Scratch, "JetBrains-Scratches", r"%APPDATA%\JetBrains", Location, "Je IDE ein Ordner scratches; nur Namen werden gelesen", Some(ScopeKind::ProjectSearch), None),
            finding("ai:claude", AiConfig, "Claude Code", r"%USERPROFILE%\.claude", Location, "Inhalt ungelesen; Zugangsdaten und Verläufe werden nie gelesen", Some(ScopeKind::UserAi), Some(7)),
            finding("ai:codex", AiConfig, "Codex", r"%USERPROFILE%\.codex", Location, "Inhalt ungelesen; Zugangsdaten und Verläufe werden nie gelesen", Some(ScopeKind::UserAi), None),
            finding("ai:agents", AiConfig, "Agent Skills (persönlich)", r"%USERPROFILE%\.agents", Location, "Inhalt ungelesen; Zugangsdaten und Verläufe werden nie gelesen", Some(ScopeKind::UserAi), None),
            finding("ai:vscode", AiConfig, "VS Code (Benutzer-MCP)", r"%APPDATA%\Code\User", Location, "Inhalt ungelesen; Zugangsdaten und Verläufe werden nie gelesen", Some(ScopeKind::UserAi), None),
        ],
    };
    Snapshot {
        demo: false,
        sqlite_version: rusqlite_version(),
        projects,
        resources,
        ai: vec![
            ai("Claude Code", "Skill", "review · Prüft Änderungen · 2 Referenzen · nicht ausgeführt", r"C:\dev\my-app\.claude\skills\review\SKILL.md", "Projekt"),
            ai("Claude Code", "Benutzereinstellungen", "3 Berechtigungen · 1 MCP-Server (Name)", r"C:\Users\x\.claude\settings.json", "Benutzer"),
            ai("Agent Skills", "Skill", "pdf · Formulare lesen · nicht ausgeführt", r"C:\Users\x\.agents\skills\pdf\SKILL.md", "Benutzer"),
            ai("Codex", "Instruktionen", "Regeln für Mitwirkende · 51 Zeilen", r"C:\tools\dev\AGENTS.md", "Projekt"),
            ai("GitHub Copilot / VS Code", "Instruktionen", "Code-Stil · 24 Zeilen", r"D:\code\website\.github\copilot-instructions.md", "Projekt"),
            ai("Cursor", "Projektregel", "3 Regeln · Bedingungen statisch", r"D:\code\website\.cursor\rules\style.mdc", "Projekt"),
        ],
        activity: vec![
            Activity { title: "Erfassung abgeschlossen".into(), detail: "5 Bereiche · 12 neu, 3 geändert".into(), created_at: "vor 25 Min.".into(), kind: "scope".into() },
            Activity { title: "Bereinigungsplan entworfen".into(), detail: "Entwurf; nichts wurde verändert.".into(), created_at: "vor 3 Std.".into(), kind: "cleanup".into() },
            Activity { title: "Lesefreigabe erteilt".into(), detail: "KI-Konfiguration des Benutzers · neue Vergleichsbasis".into(), created_at: "gestern".into(), kind: "scope".into() },
            Activity { title: "Quellen geprüft".into(), detail: "48 geprüft · 2 abgekündigt".into(), created_at: "vor 2 Tagen".into(), kind: "updates".into() },
        ],
        scopes,
        hints,
        plans: Vec::new(),
        owners: Vec::new(),
        ecosystems: vec![EcosystemCount { ecosystem: "npm".into(), declared: 31, resolved: 248, installed: 2 }],
        storage: vec![
            StorageSlice { label: "Projektquellen".into(), bytes: Some(52_000_000_000) },
            StorageSlice { label: "Paket-Caches".into(), bytes: Some(38_000_000_000) },
            StorageSlice { label: "Temporär".into(), bytes: Some(24_000_000_000) },
            StorageSlice { label: "Umgebungen".into(), bytes: Some(18_000_000_000) },
            StorageSlice { label: "Builds / Ergebnisse".into(), bytes: Some(10_000_000_000) },
        ],
        settings: Some(Settings::default()),
        packages: report,
        discoveries: Vec::new(),
        artifacts: Vec::new(),
        disks: StorageOverview::default(),
        system,
        last_scan_at: Some(now - 25 * MINUTE),
        tidy: TidyReport::default(),
        ai_access: Vec::new(),
    }
}

fn enrich(s: &mut Snapshot, now: i64) {
    let gb = 1_000_000_000u64;
    let gib = 1_073_741_824_u64;
    let tidy_item = |rel: &str, class: TidyClass, label: &str, bytes: u64, evidence: &str| TidyItem {
        rel_path: rel.into(),
        class,
        label: label.into(),
        bytes,
        evidence: evidence.into(),
        modified_at: Some(now - 30 * DAY),
    };
    s.ai_access = vec![
        ProjectAccess {
            scope_id: 1,
            name: "my-app".into(),
            path: r"C:\dev\my-app".into(),
            audience: "Projekt".into(),
            tools: vec!["Claude Code".into(), "Cursor".into()],
            level: AccessLevel::Wide,
            findings: vec![
                "Claude Code: Standardmodus bypassPermissions – Bestätigungen oder Sandbox aus".into(),
                "1 zusätzliche Ordner außerhalb des Projekts freigegeben".into(),
                "2 Hooks führen bei Ereignissen Befehle aus".into(),
            ],
            keep_out: vec!["node_modules".into(), ".next".into(), "tests/fixtures".into()],
        },
        ProjectAccess {
            scope_id: 3,
            name: "ml-experiments".into(),
            path: r"C:\dev\ml".into(),
            audience: "Projekt".into(),
            tools: vec!["Codex".into()],
            level: AccessLevel::Open,
            findings: vec!["Kein Kontextfilter: Der Agent kann jede Datei dieses Ordners lesen".into()],
            keep_out: vec![".venv".into(), "data".into()],
        },
        ProjectAccess {
            scope_id: 2,
            name: "website".into(),
            path: r"C:\dev\website".into(),
            audience: "Projekt".into(),
            tools: vec!["Cursor".into()],
            level: AccessLevel::Limited,
            findings: vec!["Ignore-Dateien: .cursorignore".into(), "4 Lese- und Ausschlussregeln".into()],
            keep_out: vec!["node_modules".into()],
        },
    ];
    s.tidy = TidyReport {
        projects: vec![
            ProjectTidy {
                project_id: 1,
                name: "my-app".into(),
                path: r"C:\dev\my-app".into(),
                total: Some(12 * gib),
                regenerable: 9 * gib,
                review: gib,
                vcs: gib / 3,
                test_data: gib,
                items: vec![
                    tidy_item("node_modules", TidyClass::Regenerable, "Abhängigkeiten (node_modules)", 6 * gib, "package-lock.json daneben"),
                    tidy_item(".next", TidyClass::Regenerable, "Build-Ausgabe", 3 * gib, "package.json daneben"),
                    tidy_item("tests/fixtures", TidyClass::Review, "Testdaten", gib, "Kann selbst gepflegte Daten enthalten"),
                ],
                last_change: Some(now - 3 * DAY),
                complete: true,
                note: String::new(),
                checked_at: now - 10 * MINUTE,
            },
            ProjectTidy {
                project_id: 6,
                name: "old-stuff".into(),
                path: r"D:\archive\experiments".into(),
                total: Some(5 * gib),
                regenerable: 4 * gib,
                review: gib / 2,
                vcs: 0,
                test_data: 0,
                items: Vec::new(),
                last_change: Some(now - 400 * DAY),
                complete: true,
                note: String::new(),
                checked_at: now - 10 * MINUTE,
            },
        ],
        stores: vec![StoreTidy {
            scope_id: 9,
            label: "Gemeinsamer NuGet-Paketspeicher".into(),
            path: r"%USERPROFILE%\.nuget\packages".into(),
            bytes: Some(2 * gib),
            class: TidyClass::Regenerable,
            evidence: "Paket-Cache; Pakete werden bei Bedarf neu geladen".into(),
        }],
        unapproved: 2,
        checked_at: Some(now - 10 * MINUTE),
    };
    s.disks = StorageOverview {
        checked_at: Some(now - 5 * MINUTE),
        drives: vec![
            DriveUsage {
                root: "C:\\".into(),
                label: "System".into(),
                kind: "Lokales Laufwerk".into(),
                total_bytes: Some(512 * gb),
                free_bytes: Some(162 * gb),
                used_bytes: Some(350 * gb),
                project_bytes: Some(38 * gb),
                status: "Metadaten vom Betriebssystem".into(),
                searchable: true,
                search: None,
            },
            DriveUsage {
                root: "D:\\".into(),
                label: "Daten".into(),
                kind: "Lokales Laufwerk".into(),
                total_bytes: Some(2000 * gb),
                free_bytes: Some(1180 * gb),
                used_bytes: Some(820 * gb),
                project_bytes: Some(14 * gb),
                status: "Metadaten vom Betriebssystem".into(),
                searchable: true,
                search: Some(DriveSearch {
                    scope_id: 9,
                    status: ScopeStatus::Approved,
                    findings: Some(14),
                    checked_at: Some(now - 2 * HOUR),
                    partial: false,
                }),
            },
        ],
        projects: s
            .projects
            .iter()
            .map(|p| ProjectStorage {
                project_id: p.id,
                name: p.name.clone(),
                path: p.path.clone(),
                drive: p.path[..3].to_string(),
                bytes: p.bytes.map(|b| b as u64),
                share: p.bytes.map(|b| b as f64 / 5.12e9),
                scope_id: Some(p.id),
                included: true,
                note: String::new(),
            })
            .collect(),
        note: "Projektgrößen sind logische Dateigrößen.".into(),
    };
    let cand = |rel: &str, name: &str, kind: DiscoveryKind, project: &str, registered: bool| ProjectCandidate {
        scope_id: 8,
        rel_path: rel.into(),
        evidence: match kind {
            DiscoveryKind::Project => "Projektmarker: package.json, .git · Hinweise: Versionsverwaltung, Lockfile, KI-Dateien (CLAUDE.md) · Sicherheit: hoch".into(),
            DiscoveryKind::Scratchpad => "Standardname einer Projektvorlage mit Entwicklungshinweisen; Zweck unbekannt".into(),
            _ => "Dateiname".into(),
        },
        current: true,
        registered,
        name: name.into(),
        kind,
        is_dir: true,
        bytes: None,
        modified_at: Some(now - 20 * DAY),
        project_id: None,
        project: project.into(),
        project_rel_path: (kind == DiscoveryKind::Project).then(|| rel.into()),
        resource_id: None,
        confidence: if kind == DiscoveryKind::Project { 3 } else { 0 },
        path_exact: true,
    };
    s.discoveries = vec![
        cand("old/api-v1", "api-v1", DiscoveryKind::Project, "", false),
        cand("Downloads/ConsoleApp3", "ConsoleApp3", DiscoveryKind::Scratchpad, "", false),
        cand("Desktop/parser-spike", "parser-spike", DiscoveryKind::Scratchpad, "", false),
        cand("ml/notebooks/eda.ipynb", "eda.ipynb", DiscoveryKind::Notebook, "ml-experiments", false),
        cand("ml/.venv", ".venv", DiscoveryKind::Environment, "ml-experiments", false),
        cand("web/testdata", "testdata", DiscoveryKind::TestData, "website", false),
    ];
    s.artifacts = vec![
        ArtifactFinding {
            scope_id: 1,
            name: "node_modules".into(),
            rel_path: "node_modules".into(),
            category: ItemCategory::Environment,
            project: "my-app".into(),
            detail: "npm-Installation".into(),
            current: true,
        },
        ArtifactFinding {
            scope_id: 4,
            name: "target".into(),
            rel_path: "target".into(),
            category: ItemCategory::Output,
            project: "tools".into(),
            detail: "Cargo-Build".into(),
            current: true,
        },
    ];
    s.scopes.push(scope(
        8,
        None,
        ScopeKind::ProjectSearch,
        r"C:\dev",
        generation(19, GenerationState::Complete, now - 25 * MINUTE, 40, None),
        None,
    ));
}

fn show_dialog(ui: &Dashboard, d: dialogs::Dialog) {
    let store = ui.global::<Store>();
    store.set_detail_title(d.title.into());
    store.set_detail_path(d.path.into());
    store.set_detail_body(d.body.into());
    store.set_detail_rows(rows_model(
        d.rows.into_iter().map(|(l, v)| DetailRow { label: l.into(), value: v.into() }).collect(),
    ));
    store.set_detail_actions(rows_model(
        d.actions.into_iter().map(|(id, l, p)| DetailAction { id: id.into(), label: l.into(), primary: p }).collect(),
    ));
    store.set_modal(1);
}

fn rusqlite_version() -> String {
    "3.50.4".into()
}

struct Renderer {
    window: Rc<MinimalSoftwareWindow>,
    output: std::path::PathBuf,
}

impl Renderer {
    fn shot(&self, name: &str, width: u32, height: u32, scale: f32) {
        self.window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
        let (w, h) = ((width as f32 * scale).round() as usize, (height as f32 * scale).round() as usize);
        self.window.set_size(PhysicalSize::new(w as u32, h as u32));
        for _ in 0..3 {
            slint::platform::update_timers_and_animations();
            self.window.request_redraw();
            let mut pixels = vec![Rgb8Pixel::default(); w * h];
            self.window.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, w);
            });
            write_ppm(&self.output.join(format!("{name}.ppm")), &pixels, w, h);
        }
    }
}

fn main() {
    let output = std::path::PathBuf::from(std::env::args_os().nth(1).expect("output directory"));
    std::fs::create_dir_all(&output).unwrap();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(RenderPlatform(window.clone()))).unwrap();
    let renderer = Renderer { window, output };
    let ui = Dashboard::new().unwrap();
    ui.show().unwrap();
    let now = SystemClock.now();
    let mut snapshot = fixture(now);
    enrich(&mut snapshot, now);
    let store = ui.global::<Store>();
    let state = || view::ViewState { package_key: None, scanning: false };

    let pages = ["uebersicht", "projekte", "pakete", "ki", "speicher", "einstellungen"];
    for (page, name) in pages.iter().enumerate() {
        store.set_page(page as i32);
        view::render_snapshot(&ui, &snapshot, r"C:\Users\x\AppData\Local\mogumogu\inventory.sqlite3", &state());
        view::render_status(&ui, false, "Bereit · 8 Lesefreigaben");
        renderer.shot(&format!("{page:02}-{name}"), 1080, 680, 1.0);
    }

    // Tabs that replaced separate pages.
    for (page, tab, name) in [
        (1, 1, "projekte-ressourcen"),
        (1, 2, "projekte-funde"),
        (2, 1, "pakete-werkzeuge"),
        (3, 1, "ki-persoenlich"),
        (3, 2, "ki-zugriff"),
        (5, 1, "einstellungen-freigaben"),
    ] {
        store.invoke_open(page, tab);
        renderer.shot(&format!("0{page}-{name}"), 1080, 680, 1.0);
    }
    // Scratchpads with the known tool folders above the list.
    store.invoke_open(1, 2);
    store.set_candidate_filter(2);
    view::render_candidates(&ui, &snapshot);
    renderer.shot("01-projekte-funde-scratchpads", 1080, 680, 1.0);
    store.set_candidate_filter(0);
    view::render_candidates(&ui, &snapshot);
    store.invoke_open(0, 0);
    store.set_overview_tab(1);
    renderer.shot("00-uebersicht-aenderungen", 1080, 680, 1.0);
    store.set_overview_tab(0);
    show_dialog(&ui, dialogs::project(&snapshot, &snapshot.projects[0]));
    renderer.shot("16-projekt-dialog", 1080, 680, 1.0);
    show_dialog(&ui, dialogs::message("Hilfe", dialogs::HELP));
    renderer.shot("17-hilfe", 1080, 680, 1.0);
    store.set_modal(0);

    // Package explanation next to the list.
    store.set_page(2);
    view::render_packages(&ui, &snapshot.packages, Some("npm:left-pad:1.3.0"));
    store.set_selection_kind("package".into());
    store.set_selection_key("npm:left-pad:1.3.0".into());
    renderer.shot("10-pakete-erklaerung", 1080, 680, 1.0);

    // A recycle-bin plan: recoverable folders with their evidence, locked
    // until the review.
    store.invoke_open(4, 1);
    view::render_snapshot(&ui, &snapshot, "", &state());
    show_dialog(&ui, dialogs::plan(&recycle_plan(now)));
    renderer.shot("11-papierkorb-plan", 1080, 680, 1.0);
    store.set_modal(0);

    // Global search across areas.
    store.set_page(0);
    view::render_snapshot(&ui, &snapshot, "", &state());
    store.set_query("pars".into());
    view::render_search(&ui, &[
        SearchHit { area: SearchArea::Resource, key: "20".into(), title: "parser-spike".into(), detail: r"Scratchpad · C:\dev\scratch\parser-spike".into() },
        SearchHit { area: SearchArea::Package, key: "parse-json".into(), title: "parse-json".into(), detail: "npm · 2 Versionen".into() },
        SearchHit { area: SearchArea::Ai, key: "0".into(), title: "Claude Code · Skill".into(), detail: r"Projekt · C:\dev\my-app\.claude\skills\parser\SKILL.md".into() },
    ]);
    store.set_search_open(true);
    renderer.shot("12-suche", 1080, 680, 1.0);
    store.set_search_open(false);
    store.set_query("".into());

    // Scanning: Mochi chews, progress and a safe stop.
    view::render_progress(&ui, &ScanProgress { running: true, cancelling: false, entries: 18_432, done: 2, total: 5 });
    view::render_snapshot(&ui, &snapshot, "", &view::ViewState { package_key: None, scanning: true });
    renderer.shot("13-erfassung-laeuft", 1080, 680, 1.0);
    view::render_progress(&ui, &ScanProgress::default());

    // The strip keeps the result of a pass the user started.
    let result = serde_json::json!([{ "scope_id": 1, "state": "complete", "baseline": false, "added": 0, "removed": 0, "changed": 0, "problem": null }]);
    let (text, kind) = view::scan_notice(&result, &snapshot);
    view::render_scan_notice(&ui, &text, kind);
    view::render_snapshot(&ui, &snapshot, "", &state());
    renderer.shot("15-erfassung-fertig", 1080, 680, 1.0);
    // A drive search links to its findings.
    store.set_page(4);
    view::render_scan_notice(&ui, "Laufwerk D: durchsucht · 14 Funde.", "ok");
    view::render_scan_link(&ui, "Funde ansehen", 1, 2);
    renderer.shot("18-laufwerk-durchsucht", 1080, 680, 1.0);
    view::render_scan_notice(&ui, "", "");

    // "Projekt hinzufügen" in steps, on an empty inventory.
    let empty = Snapshot { system: snapshot.system.clone(), settings: snapshot.settings.clone(), ..Default::default() };
    store.set_page(0);
    view::render_snapshot(&ui, &empty, "", &state());
    renderer.shot("20-uebersicht-leer", 1080, 680, 1.0);
    store.set_guide_path(r"C:\dev\api-service".into());
    store.set_guide_name("api-service".into());
    store.set_guide_open(true);
    for (step, name) in [(1, "ordner"), (2, "freigabe"), (3, "erfassung"), (4, "fertig")] {
        store.set_guide_step(step);
        if step == 3 {
            view::render_progress(&ui, &ScanProgress { running: true, cancelling: false, entries: 1_248, done: 0, total: 1 });
        }
        if step == 4 {
            view::render_progress(&ui, &ScanProgress::default());
            store.set_guide_result("Vollständig erfasst. Die Vergleichsbasis steht; Hinweise brauchen Beobachtungszeit.".into());
        }
        renderer.shot(&format!("2{step}-einstieg-{name}"), 1080, 680, 1.0);
    }
    store.set_guide_open(false);

    // Smallest window and high DPI.
    store.set_page(0);
    view::render_snapshot(&ui, &snapshot, "", &state());
    renderer.shot("30-uebersicht-900x560", 900, 560, 1.0);
    store.set_page(4);
    renderer.shot("31-speicher-900x560", 900, 560, 1.0);
    store.invoke_open(4, 1);
    renderer.shot("04-speicher-aufraeumen", 1080, 680, 1.0);
    store.invoke_open(4, 0);
    store.set_page(0);
    renderer.shot("32-uebersicht-150", 1080, 680, 1.5);
    println!("Native renders written; synthetic data only.");
}

fn rows_model<T: Clone + 'static>(items: Vec<T>) -> slint::ModelRc<T> {
    Rc::new(slint::VecModel::from(items)).into()
}
