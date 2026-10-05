---
name: mogumogu-performance
description: Analysiere Tray-Overhead, Speicherwachstum, Watcherlast und Scanner-Performance in mogumogu. Verwenden für Messungen, Worker-/Queue-Design und Dashboard-Lebenszyklen.
metadata:
  version: "0.1"
  project: "mogumogu"
---

# mogumogu-performance

## Vorgehen

Lies `AGENTS.md`, `src/limits.rs`, `src/owner.rs` und die Performanceziele im Plan. Halte Ziele und Messergebnisse strikt auseinander. Dokumentiere Windows-Version, Hardware, Fixture, optimierten Build, Softwareversionen, Messdauer und aktive Hintergrundaufgaben.

Messe Tray vor dem ersten Fenster, geöffnetes Dashboard und Rückkehr nach dem Schließen getrennt. Betrachte Private Bytes, Working Set, Handles, Threads, CPU-Zeit und I/O. Kurzlebige Manager-/CLI-Prozesse in Lastmessungen nicht verstecken.

Prüfe begrenzte Queues, Ereignisbündelung, Overflow-Reconciliation und Abbruchfähigkeit. Keine Vollscans pro Datei, keine permanente Größenberechnung und keine separate unbeschränkte KI-Indexierung.

Teste 100 Fensterzyklen, große Metadaten, viele gemeinsame Beziehungen, Skillprofile und Installationsereignisse. Verwende identische Fixtures für Vergleiche, berichte Streuung und Regressionen.

Reduziere Arbeit und Allokation, bevor Sicherheitsprüfungen verändert werden. Nie Schutzmaßnahmen abschalten, um Budgets optisch zu erreichen. Verfehlt ein Budget das Ziel, dokumentiere Ursache und einen überprüfbaren Änderungsvorschlag.

## Ergebnis

Liefere reproduzierbares Messverfahren, echte Daten, verbleibende Grenzen und Wirkung der Änderung. Ein kleines Demo-Fenster ersetzt keinen Inventartest.

## Prüfbefehle

- `cargo test --release --no-default-features --test performance -- --ignored --nocapture`
- `.\scripts\measure.ps1 -Cycles 100 -IdleSeconds 600`
