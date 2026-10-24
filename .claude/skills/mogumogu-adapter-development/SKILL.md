---
name: mogumogu-adapter-development
description: Entwickle oder prüfe mogumogu-Paketadapter und statische Dateiprofile. Verwenden bei neuen Paketmanagern, Parsern, Inventarbeziehungen, Versionstests oder Adapter-Capabilities.
metadata:
  version: "0.1"
  project: "mogumogu"
---

# mogumogu-adapter-development

## Vorgehen

Lies zuerst `AGENTS.md`, `docs/IMPLEMENTIERUNGSPLAN.md`, `docs/UNTERSTUETZUNGSMATRIX.md` und `src/adapters/mod.rs` sowie den konkreten Quellvertrag des Managers. Unterscheide eine deklarative Dateierkennung von komplexer Managersemantik. Lege ID, Quellen, getestete Versionen und die getrennten Fähigkeiten fest.

Beginne mit statischen positiven und negativen Fixtures. Modelliere deklarierte, aufgelöste und installierte Zustände getrennt. Behalte Quelle, Workspace, Features, Zielplattform oder Framework, soweit bekannt. Fehlende Informationen werden nicht erraten.

Verwende kontrollierte Programme nur in ausdrücklich unterstützten Aufrufkontexten. Inventarisierung darf keine Installation, Restore, Buildskripte, Lifecycle-Hooks oder Projektplugins auslösen. Netzwerk ist eine separat freigegebene Fähigkeit. Ausgabe und Fehler bleiben begrenzt und redigiert.

Teste Baseline, Änderungen, nicht verfügbare Manager, private Quellen, Offlinebetrieb, beschädigte/große Dateien, Versionswechsel, gemeinsame Stores und doppelte Installationsnachweise. Dokumentiere, welche Einschränkungen eine partielle Darstellung bewirken.

## Fertigkriterium

Ein Adapter-Patch liefert Parser/Logik, Fixtures, Capability-/Versionsmatrix, Datenschutz-/Kostenhinweise und eine überprüfte Quelle. Inventarunterstützung verleiht keine Löschfähigkeit; entsprechende Änderungen brauchen einen separaten Cleanup-Review.

## Prüfbefehle

- `cargo test --no-default-features --lib adapters`
- `cargo test --no-default-features --test safe_reading`
