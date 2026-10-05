//! Separate demo database seeded from `assets/demo.json`, the fixture shared
//! with the HTML design preview. Demo rows are never written to the local
//! inventory and the demo database accepts no registrations.
use super::Repository;
use crate::Result;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

pub const FIXTURE: &str = include_str!("../../assets/demo.json");

#[derive(Debug, Deserialize)]
struct Fixture {
    projects: Vec<FixtureProject>,
    resources: Vec<FixtureResource>,
    ai: Vec<FixtureAi>,
    activity: Vec<FixtureActivity>,
}

#[derive(Debug, Deserialize)]
struct FixtureProject {
    id: i64,
    name: String,
    path: String,
    ecosystem: String,
    state: String,
    evidence: String,
    bytes: Option<i64>,
    packages: Option<i64>,
    protected: bool,
}

#[derive(Debug, Deserialize)]
struct FixtureResource {
    id: i64,
    project_id: Option<i64>,
    name: String,
    path: String,
    kind: String,
    bytes: Option<i64>,
    state: String,
    evidence: String,
    protected: bool,
}

#[derive(Debug, Deserialize)]
struct FixtureAi {
    client: String,
    label: String,
    path: String,
    state: String,
}

#[derive(Debug, Deserialize)]
struct FixtureActivity {
    title: String,
    detail: String,
    created_at: String,
}

impl Repository {
    pub(super) fn seed_demo(&mut self) -> Result<()> {
        let fixture: Fixture = serde_json::from_str(FIXTURE)?;
        self.write(|tx| {
            let seeded: Option<String> =
                tx.query_row("SELECT value FROM app_meta WHERE key='seeded'", [], |r| r.get(0)).optional()?;
            if seeded.is_some() {
                return Ok(());
            }
            for p in &fixture.projects {
                tx.execute(
                    "INSERT INTO projects(id,name,path,ecosystem,state,evidence,bytes,packages,protected,origin)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'demo')",
                    params![p.id, p.name, p.path, p.ecosystem, p.state, p.evidence, p.bytes, p.packages, p.protected],
                )?;
            }
            for r in &fixture.resources {
                tx.execute(
                    "INSERT INTO resources(id,project_id,name,path,kind,bytes,state,evidence,protected,origin)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'demo')",
                    params![r.id, r.project_id, r.name, r.path, r.kind, r.bytes, r.state, r.evidence, r.protected],
                )?;
            }
            for a in &fixture.ai {
                tx.execute(
                    "INSERT INTO ai_configs(client,label,path,state) VALUES(?1,?2,?3,?4)",
                    params![a.client, a.label, a.path, a.state],
                )?;
            }
            for a in fixture.activity.iter().rev() {
                tx.execute(
                    "INSERT INTO activity(title,detail,created_at,kind) VALUES(?1,?2,?3,'demo')",
                    params![a.title, a.detail, a.created_at],
                )?;
            }
            tx.execute("INSERT INTO app_meta(key,value) VALUES('seeded','1')", [])?;
            Ok(())
        })
    }

    /// Demo AI rows (the local inventory derives AI entries from scans).
    pub fn demo_ai(&mut self) -> Result<Vec<crate::domain::AiEntry>> {
        self.read(|tx| {
            let mut statement = tx.prepare("SELECT client,label,path,state FROM ai_configs ORDER BY id LIMIT 200")?;
            let rows = statement.query_map([], |r| {
                Ok(crate::domain::AiEntry {
                    client: r.get(0)?,
                    label: r.get(1)?,
                    path: r.get(2)?,
                    state: r.get(3)?,
                    kind: "Beispiel".into(),
                    scope: "Projekt".into(),
                    references: Vec::new(),
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }
}
