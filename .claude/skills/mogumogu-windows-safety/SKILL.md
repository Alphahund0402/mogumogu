---
name: mogumogu-windows-safety
description: Prüfe Windows-10/11-spezifische Sicherheit und Lebenszyklen in mogumogu. Verwenden für Pfadidentität, Junctions, Reparse Points, IPC, Prozesse, Handles und destruktive Traversierung.
metadata:
  version: "0.1"
  project: "mogumogu"
---

# mogumogu-windows-safety

## Vorgehen

Lies `AGENTS.md`, `src/platform/windows.rs`, `src/fsread.rs` und das Sicherheitskapitel. Lege die getesteten Windows-Builds und Dateisystemannahmen fest. Prüfe nicht nur Pfadtexte, sondern sichere Identität und genehmigte Grenzen zum Zeitpunkt der Aktion.

Berücksichtige Austausch nach Planung, umbenannte Verzeichnisse, Links/Junctions/Reparse Points, geschützte Kinder, Volumewechsel, lange Pfade, abweichende Groß-/Kleinschreibregeln und Berechtigungsfehler. Eine einmalige Canonicalize- oder Präfixprüfung ist keine vollständige Absicherung.

Prüfe Prozesse nicht nur per PID; verwende passende Lebenszyklusinformationen. Unsichere aktive Sessions blockieren. Kein automatisches Beenden fremder Prozesse und keine pauschale Elevation.

Lokale IPC benötigt begrenzte Nachrichten und explizite Zugriffsrechte. Ein anderer Prozess desselben kompromittierten Benutzers bleibt eine gesonderte Bedrohung; Named Pipes und einzelne Prozesse sind keine universelle Sandbox.

Lösch- und Race-Tests ausschließlich in entbehrlichen Testwurzeln ausführen. Teste Abbruch, Fehler zwischen Validierung und Aktion sowie Wiederanlauf. Tray-Lifecycle auf Explorer-Neustart, Sleep/Resume, DPI und Mehrmonitor prüfen.

## Ergebnis

Liefere getestete Schutzfälle, nicht abgedeckte Situationen, API-/Versionsannahmen und minimale Fehlerreproduktionen. Unsichere Fälle bleiben schreibgeschützt.

## Prüfbefehle

- `cargo test --no-default-features --test safe_reading`
- `cargo test --no-default-features --test platform`
