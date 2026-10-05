---
name: mogumogu-cleanup-review
description: Führe einen Sicherheitsreview von mogumogu-Bereinigungsplänen und destruktiven Änderungen durch. Verwenden vor Einführung von Löschfähigkeit, Stilllegung, Quarantäne oder Recovery-Retry.
metadata:
  version: "0.1"
  project: "mogumogu"
---

# mogumogu-cleanup-review

## Prüfreihenfolge

Lies `AGENTS.md`, `src/cleanup/`, `src/platform/windows.rs` (Löschen per Handle) und den zu prüfenden Diff. Der Plan muss Ressourcenidentität, Operation, Grund, Besitzer, erhaltene Inhalte, Blocker und Wiederherstellungsgrenzen zeigen. Eine Plan-ID autorisiert keinen beliebigen späteren Pfadinhalt.

Prüfe, dass Alter, Namesmuster, fehlende Nutzung, nicht installierter KI-Client oder Duplikatbefund allein keine Bereinigung erlauben. Geschützte Kinder, Quellen, Settings, Ergebnisse und unbekannte Daten müssen erhalten bleiben.

Verlange erneute Identitäts-/Grenzprüfung, aktive Nutzungssperren, Manager-Locks im unterstützten Umfang, sichere Traversierung, dauerhaftes Journal und pro-Ziel-Fehler. Prüfe mögliche Zeitfenster für Austausch oder neue Dateien.

Quarantäne, Backup und Rekonstruktionsrezept sind unterschiedliche Mechanismen. Quarantäne gibt nicht sofort Speicher frei. Kein stiller Fallback von fehlgeschlagener Quarantäne zu endgültigem Löschen.

Prüfe unterbrochene Aktionen mit Fehlereinbringung. Ein Retry auf veränderte Ziele ist neu freizugeben. UI und CLI müssen dieselben Kernprüfungen verwenden.

## Ergebnis

Berichte blockierende Datenverlustpfade mit konkreter Reproduktion und Schwere, danach nichtblockierende Verbesserungen. Teste ausschließlich entbehrliche Fixtures. Freigabe nur für den ausdrücklich geprüften Ressourcentyp und die dokumentierte Plattformmatrix.

## Prüfbefehle

- `cargo test --no-default-features --test core crash`
- `cargo test --no-default-features --features network --test end_to_end`
