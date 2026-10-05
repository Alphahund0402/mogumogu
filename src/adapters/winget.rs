//! WinGet context: the Windows "installed programs" list (HKCU/HKLM
//! Uninstall keys), read-only. `winget list` is not started because it may
//! update sources over the network. Presence in this list says nothing about
//! which manager installed or manages a program; that stays unknown.
use super::*;
use crate::domain::{InstallState, InventoryItem, ItemCategory};

pub struct WinGet;

static DESCRIPTOR: Descriptor = Descriptor {
    id: "winget",
    name: "WinGet",
    markers: &[],
    scope: AdapterScope::System,
    inventory: Support::Limited,
    updates: Support::Unsupported,
    cleanup: Support::Unsupported,
    formats: &["Registry: Uninstall-Schlüssel HKCU, HKLM (64/32 Bit)"],
    limits: "Nur installierte Programme laut Windows; WinGet-Verwaltung und -Quelle werden nicht aus Namen abgeleitet.",
};

impl Adapter for WinGet {
    fn descriptor(&self) -> &'static Descriptor {
        &DESCRIPTOR
    }
}

impl WinGet {
    /// Reads the installed-programs list. The second value is `true` when
    /// the entry limit truncated the list.
    pub fn inventory_registry(&self, max: usize) -> Result<(Vec<InventoryItem>, bool), String> {
        let (entries, truncated) = crate::platform::uninstall_entries(max).map_err(|e| e.to_string())?;
        let items = entries
            .into_iter()
            .map(|entry| {
                let publisher = entry.publisher.as_deref().unwrap_or("unbekannt");
                InventoryItem::new(ItemCategory::Software, "winget", entry.name, entry.hive)
                    .version(entry.version.unwrap_or_default())
                    .state(InstallState::Installed)
                    .detail(format!(
                        "Windows-Softwareliste · Herausgeber {publisher} · Verwaltung durch WinGet nicht belegt"
                    ))
            })
            .collect();
        Ok((items, truncated))
    }
}
