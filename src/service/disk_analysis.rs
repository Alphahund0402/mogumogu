//! Explicit disk analysis and cached paging shared by native UI and CLI.
use super::Core;
use crate::domain::*;
use crate::fsread::{RootError, ScopeRoot};
use crate::inventory::disk::DiskIndex;
use crate::limits::ScanLimits;
use crate::{Error, Result};
use std::path::Path;

impl Core {
    pub fn analyze_storage(&mut self, scope_id: i64) -> Result<DiskPage> {
        self.repo.require_local()?;
        let scope = self.analysis_scope(scope_id)?;
        let own = scope.kind == ScopeKind::Storage;
        // A failed refresh must not leave an older scan looking current.
        self.disk_index = None;
        let identity = self
            .repo
            .scope_identity(scope_id)?
            .ok_or_else(|| Error::blocked("Ordneridentität fehlt; Speicheranalyse bitte neu freigeben."))?;
        let root = match ScopeRoot::open_local(Path::new(&scope.path), Some(identity)) {
            Ok(root) => root,
            // A refusal says nothing about the folder: a project on a network
            // drive keeps its own read approval and status.
            Err(RootError::Unsafe(_)) => {
                return Err(Error::blocked(
                    "Speicheranalyse nur auf lokalen Festplatten und ohne Verknüpfung im Pfad möglich.",
                ));
            }
            Err(RootError::IdentityChanged) => {
                let message = "Ordner wurde ersetzt; bitte neu freigeben.";
                self.repo.set_scope_status(scope_id, ScopeStatus::IdentityChanged, Some(message))?;
                return Err(Error::blocked(message));
            }
            // Reachability of a project is the inventory's to record: a project
            // marked unavailable here would drop out of "scan all".
            Err(RootError::Unavailable(_)) => {
                let message = "Ordner derzeit nicht erreichbar; später erneut analysieren.";
                if own {
                    self.repo.set_scope_status(scope_id, ScopeStatus::Unavailable, Some(message))?;
                }
                return Err(Error::blocked(message));
            }
        };
        if own && scope.status != ScopeStatus::Approved {
            self.repo.set_scope_status(scope_id, ScopeStatus::Approved, None)?;
        }
        self.scan_control.begin(1);
        let mut index = DiskIndex::scan(
            scope_id,
            &scope.path,
            root,
            self.repo.now(),
            ScanLimits::storage_analysis(),
            Some(self.scan_control()),
        );
        self.scan_control.scope_done();
        self.scan_control.finish();
        // Unique across owner restarts: seconds, then a counter within one.
        self.disk_runs += 1;
        index.run = ((self.repo.now().max(1) as u64) << 16) | (self.disk_runs & 0xFFFF);
        let page = index.page(&DiskQuery::default())?;
        self.disk_index = Some(index);
        Ok(page)
    }

    /// Queries never touch the filesystem or start a scan implicitly.
    pub fn storage_page(&mut self, scope_id: i64, query: &DiskQuery) -> Result<DiskPage> {
        self.analysis_scope(scope_id)?;
        let index =
            self.disk_index.as_ref().filter(|i| i.scope_id == scope_id).ok_or_else(|| {
                Error::blocked("Kein Analysestand für diesen Bereich. Speicheranalyse zuerst starten.")
            })?;
        index.page(query)
    }

    pub fn holds_storage_analysis(&self) -> bool {
        self.disk_index.is_some()
    }

    /// Frees the index; the next page request asks for a new analysis.
    pub fn clear_storage_analysis(&mut self) {
        self.disk_index = None;
    }

    fn analysis_scope(&mut self, scope_id: i64) -> Result<Scope> {
        let scope = self.repo.scope(scope_id)?;
        if scope.analyzable() {
            return Ok(scope);
        }
        Err(Error::blocked(match scope.status {
            ScopeStatus::Revoked => "Lesefreigabe wurde widerrufen.",
            ScopeStatus::IdentityChanged => "Der freigegebene Ordner wurde ersetzt. Bitte bewusst neu freigeben.",
            _ => {
                "Speicheranalyse braucht eine eigene Freigabe oder die Lesefreigabe eines Projekts. Eine Namenssuche, KI- oder Cache-Freigabe reicht nicht."
            }
        }))
    }
}
