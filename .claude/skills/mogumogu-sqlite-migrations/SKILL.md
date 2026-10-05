---
name: mogumogu-sqlite-migrations
description: Entwirf und prüfe SQLite-Schemaänderungen, Migrationen, Backups und Aktionsjournale für mogumogu. Verwenden bei Datenhaltung, Schreibkoordination oder Crash-Recovery.
metadata:
  version: "0.1"
  project: "mogumogu"
---

# mogumogu-sqlite-migrations

## Vorgehen

Lies `AGENTS.md`, `migrations/`, `src/storage/` und das Datenkapitel. Prüfe Schema-/Engine-Versionen, lokale Datenablage und den zentral koordinierten Schreibpfad. Halte Ressourcenidentität, Orte, Besitzer, Aktivität und Bereinigungsstatus unabhängig.

Entwirf Migration und Fehlerpfad zusammen. Sichere konsistent, teste alte Datenstände sowie eine ältere App mit unbekannt neuerem Schema. Niemals eine produktive geöffnete Datenbank blind als konsistentes Backup kopieren.

Aktionszustand muss vor relevanten Dateisystemänderungen dauerhaft gespeichert sein. SQLite-Commit und Dateilöschung bilden keine gemeinsame Transaktion. Plane deshalb Idempotenz, Zustandsabgleich und per-Ziel-Ergebnisse statt blindem Retry.

Teste Fehler vor Journal-Commit, nach Commit, während der Aktion und vor Ergebnisaufzeichnung. Bei geänderter Ressourcenidentität darf kein alter Auftrag wiederverwendet werden.

Persistiere keine Roh-Secrets oder sensitiven Settings-Diffs. Begrenze Historie und Caches, aber lösche keine für ausstehende Recovery benötigten Datensätze.

## Ergebnis

Liefere Schema/Migration, Indizes/Abfrageplan soweit relevant, Backup-/Recovery-Test und dokumentierte Unbekannt-/Teilzustände. Keine universelle Rückgängig-Zusage.

## Prüfbefehle

- `cargo test --no-default-features --test core migrat`
- `cargo test --no-default-features --test core secrets`
