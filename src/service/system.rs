//! System findings: metadata of the search path and known places, matched
//! with the stored read approvals. Checked on first use and on request, not
//! on every dashboard refresh; nothing here reads content or starts tools.
use super::Core;
use crate::Result;
use crate::domain::{Scope, ScopeKind, SystemReport};
use crate::inventory::system::{self, MetadataProbe, ProcessEnv};

impl Core {
    /// The system report, checked again when `refresh` is set.
    pub fn system_report(&mut self, refresh: bool) -> Result<SystemReport> {
        if self.is_demo() {
            return Ok(SystemReport::default());
        }
        let scopes = self.repo.scopes()?;
        // A new check changes what the dashboard shows without a write.
        self.changed |= refresh;
        Ok(with_scopes(self.checked_system(refresh), &scopes))
    }

    /// The cached report for the dashboard; only the first call checks.
    pub(super) fn system_with_basis(&mut self, scopes: &[Scope]) -> SystemReport {
        if self.is_demo() {
            return SystemReport::default();
        }
        with_scopes(self.checked_system(false), scopes)
    }

    fn checked_system(&mut self, refresh: bool) -> SystemReport {
        if refresh || self.system.is_none() {
            let mut report = system::discover(&ProcessEnv, &MetadataProbe);
            report.checked_at = Some(self.repo.now());
            self.system = Some(report);
        }
        self.system.clone().unwrap_or_default()
    }
}

/// Marks findings whose place already has an approval of the matching kind.
fn with_scopes(mut report: SystemReport, scopes: &[Scope]) -> SystemReport {
    for finding in &mut report.findings {
        let Some(kind) = finding.scope else { continue };
        let scope = scopes
            .iter()
            .find(|s| s.kind == kind && (kind == ScopeKind::Winget || s.path.eq_ignore_ascii_case(&finding.path)));
        if let Some(scope) = scope {
            finding.scope_id = Some(scope.id);
            finding.coverage = scope.coverage_label();
        }
    }
    report
}
