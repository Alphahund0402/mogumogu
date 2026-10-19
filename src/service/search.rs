//! One search box for the dashboard and `search` in the CLI: bounded hits
//! from stored metadata only. A hit names where an entry lives; opening it
//! is a separate, read-only step.
use super::Core;
use crate::Result;
use crate::domain::*;
use crate::limits::UI_PAGE;

/// Hits shown per area; the dashboard lists them in one bounded popup.
pub const SEARCH_PER_AREA: usize = 8;

impl Core {
    pub fn search(&mut self, text: &str) -> Result<Vec<SearchHit>> {
        let needle = text.trim().to_lowercase();
        if needle.chars().count() < 2 {
            return Ok(Vec::new());
        }
        let contains = |value: &str| value.to_lowercase().contains(&needle);
        let mut hits = Vec::new();
        for project in self.repo.projects()?.into_iter().filter(|p| contains(&p.name) || contains(&p.path)) {
            hits.push(SearchHit {
                area: SearchArea::Project,
                key: project.id.to_string(),
                title: project.name,
                detail: project.path,
            });
            if hits.len() >= SEARCH_PER_AREA {
                break;
            }
        }
        for (ecosystem, name, versions) in self.repo.search_packages(&needle, SEARCH_PER_AREA)? {
            hits.push(SearchHit {
                area: SearchArea::Package,
                detail: format!("{ecosystem} · {versions} Version{}", if versions == 1 { "" } else { "en" }),
                key: name.clone(),
                title: name,
            });
        }
        let resources = self.repo.resources()?;
        hits.extend(
            resources.into_iter().filter(|r| contains(&r.name) || contains(&r.path)).take(SEARCH_PER_AREA).map(|r| {
                SearchHit {
                    area: SearchArea::Resource,
                    key: r.id.to_string(),
                    detail: format!("{} · {}", r.kind.label(), r.path),
                    title: r.name,
                }
            }),
        );
        for (scope_id, item) in self.repo.search_discoveries(&needle, SEARCH_PER_AREA)? {
            let kind = item.evidence.discovery.as_ref().map_or(DiscoveryKind::Project, |d| d.kind);
            hits.push(SearchHit {
                area: SearchArea::Discovery,
                key: format!("{scope_id}:{}", item.rel_path),
                title: item.name,
                detail: format!("{} · {}", kind.label(), if item.rel_path.is_empty() { "." } else { &item.rel_path }),
            });
        }
        let ai: Vec<AiEntry> = if self.is_demo() {
            self.repo.demo_ai()?
        } else {
            let scopes = self.repo.scopes()?;
            let mut items = self.repo.published_items_all(Some(ItemCategory::AiArtifact), UI_PAGE)?;
            items.extend(self.repo.published_items_all(Some(ItemCategory::AiReference), UI_PAGE * 10)?);
            super::snapshot::ai_entries(&items, &scopes)
        };
        // The key is the position in the dashboard's AI list of this revision.
        hits.extend(
            ai.into_iter()
                .enumerate()
                .filter(|(_, a)| contains(&a.client) || contains(&a.kind) || contains(&a.label) || contains(&a.path))
                .take(SEARCH_PER_AREA)
                .map(|(index, a)| SearchHit {
                    area: SearchArea::Ai,
                    key: index.to_string(),
                    title: format!("{} · {}", a.client, a.kind),
                    detail: format!("{} · {}", a.scope, a.path),
                }),
        );
        if let Some(system) = &self.system {
            hits.extend(
                system
                    .findings
                    .iter()
                    .filter(|f| contains(&f.name) || contains(&f.location))
                    .take(SEARCH_PER_AREA)
                    .map(|f| SearchHit {
                        area: SearchArea::System,
                        key: f.id.clone(),
                        title: f.name.clone(),
                        detail: format!("{} · {}", f.evidence.label(), f.location),
                    }),
            );
        }
        Ok(hits)
    }
}
