//! Package-review use cases: bounded data access, page caching and Keep decisions.
//! Pure evidence calculation lives in `analysis`; matching lives in `references`.
mod analysis;
mod references;

use super::Core;
use crate::domain::*;
use crate::limits::{PACKAGE_ANALYSIS_ITEMS as ANALYSIS_LIMIT, PACKAGE_PAGE};
use crate::{Error, Result};
use analysis::{PackageInput, analyze};

/// One page, exact for this repository revision.
#[derive(Debug)]
pub(super) struct CachedPackages {
    revision: u64,
    query: PackageQuery,
    offset: usize,
    limit: usize,
    report: PackageReport,
}

impl Core {
    pub fn package_report(&mut self, offset: usize, limit: usize) -> Result<PackageReport> {
        self.package_page(&PackageQuery::default(), offset, limit)
    }

    /// One page of findings for a text, filter and manager.
    pub fn package_page(&mut self, query: &PackageQuery, offset: usize, limit: usize) -> Result<PackageReport> {
        if self.is_demo() {
            return Ok(PackageReport::default());
        }
        if let Some(report) = self.cached_packages(query, offset, limit) {
            return Ok(report);
        }
        let scopes = self.repo.scopes()?;
        let projects = self.repo.projects()?;
        self.packages_with_basis(query, offset, limit, &scopes, &projects)
    }

    fn cached_packages(&self, query: &PackageQuery, offset: usize, limit: usize) -> Option<PackageReport> {
        self.package_cache
            .as_ref()
            .filter(|cache| {
                cache.revision == self.repo.revision()
                    && cache.query == *query
                    && cache.offset == offset
                    && cache.limit == limit.clamp(1, PACKAGE_PAGE)
            })
            .map(|cache| cache.report.clone())
    }

    pub(super) fn packages_with_basis(
        &mut self,
        query: &PackageQuery,
        offset: usize,
        limit: usize,
        scopes: &[Scope],
        projects: &[Project],
    ) -> Result<PackageReport> {
        if self.is_demo() {
            return Ok(PackageReport::default());
        }
        if let Some(report) = self.cached_packages(query, offset, limit) {
            return Ok(report);
        }
        let revision = self.repo.revision();
        let limit = limit.clamp(1, PACKAGE_PAGE);
        let items = self.repo.published_items_all(None, ANALYSIS_LIMIT + 1)?;
        let retained = self.repo.retained_packages()?;
        let report =
            analyze(PackageInput { items: &items, scopes, projects, retained: &retained }, query, offset, limit);
        self.package_cache =
            Some(CachedPackages { revision, query: query.clone(), offset, limit, report: report.clone() });
        Ok(report)
    }

    pub fn retain_package(&mut self, key: &str, retained: bool) -> Result<()> {
        let items = self.repo.published_items_all(None, ANALYSIS_LIMIT + 1)?;
        if !items.iter().any(|p| p.item.package_id().key() == key) {
            return Err(Error::not_found("Paket nicht im freigegebenen Inventar gefunden."));
        }
        self.repo.retain_package(key, retained)
    }
}
