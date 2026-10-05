# Herkunft und Lizenzhinweise

- Quellcode, eigene UI-Elemente, Icons, Logo und Dokumentation: GPL-3.0-only (`LICENSE`).
- Slint wird über den GPL-3.0-only-Lizenzpfad eingebunden.
- SQLite ist gemeinfrei und wird über rusqlite (`bundled`) eingebunden; tatsächlich gebaute
  Engine: 3.53.2.
- Weitere Rust-Abhängigkeiten (u. a. serde, toml, quick-xml, regex, notify, sha2, ureq mit
  native-tls/SChannel, windows-sys) behalten ihre eigenen, GPL-kompatiblen Lizenzen
  (überwiegend MIT/Apache-2.0). `scripts/release.ps1` erzeugt je Release ein vollständiges
  Abhängigkeits- und Lizenzinventar (`dependencies.json`, `licenses.txt`) aus `Cargo.lock`.
  Dieses Dokument ist kein SBOM.
- `assets/mountains.jpg` ist ein bereinigter Ausschnitt der vom Auftraggeber bereitgestellten
  Dashboard-Referenz. Die externen Verwertungsrechte sind nicht unabhängig geprüft; vor
  öffentlicher Distribution die Freigabe dokumentieren oder ein selbst lizenziertes Motiv verwenden.
- Logo und Linienicons (`assets/*.svg`) sind für dieses Projekt neu gezeichnet; keine
  Markenlogos von KI-Anbietern. `assets/tray.png` ist daraus abgeleitet.
- Keine Fontdateien im Repository; die Anwendung nutzt vorhandene Systemschriften (Segoe UI).
- `docs/screenshots/` zeigen die native Anwendung mit Demodaten bzw. diesem Repository.

Dokumentation der verwendeten APIs: `docs/QUELLEN.md`.
