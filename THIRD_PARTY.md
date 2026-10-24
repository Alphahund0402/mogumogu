# Herkunft und Lizenzhinweise

- Quellcode, eigene UI-Elemente, Icons, Logo und Dokumentation: GPL-3.0-only (`LICENSE`).
- Slint wird über den GPL-3.0-only-Lizenzpfad eingebunden.
- SQLite ist gemeinfrei und wird über rusqlite (`bundled`) eingebunden; tatsächlich gebaute
  Engine: 3.53.2.
- Weitere Rust-Abhängigkeiten (u. a. serde, toml, quick-xml, regex, notify, sha2, semver,
  pep440_rs für PEP-440-Versionen, ureq mit
  native-tls/SChannel, windows-sys und der Buildhelfer embed-resource) behalten ihre eigenen, GPL-kompatiblen Lizenzen
  (überwiegend MIT/Apache-2.0). `scripts/release.ps1` erzeugt je Release ein vollständiges
  Abhängigkeits- und Lizenzinventar (`dependencies.json`, `licenses.txt`) aus `Cargo.lock`.
  Dieses Dokument ist kein SBOM.
- `assets/mountains.jpg` ist ein bereinigter Ausschnitt der vom Auftraggeber bereitgestellten
  Dashboard-Referenz. Die externen Verwertungsrechte sind nicht unabhängig geprüft; vor
  öffentlicher Distribution die Freigabe dokumentieren oder ein selbst lizenziertes Motiv verwenden.
- Die Linienicons (`assets/icons/*.svg`) und das frühere Logo (`assets/logo.svg`)
  sind für dieses Projekt neu gezeichnet; keine Markenlogos von KI-Anbietern.
- Die Mochi-Identität (`assets/brand/`, `assets/tray.png`) basiert auf der vom
  Auftraggeber ausgewählten KI-generierten Konzeptstudie. Icon und Wortmarke
  sind Originalausschnitte; `mascots-v2.png` ist die mit `image_gen` vereinheitlichte
  Fassung der vier Posen. `mascots-v3-moods.png` (Mampft, Schläft, Besorgt) ist eine
  deterministische Pixelbearbeitung von `mascots-v2.png` mit
  `design/brand/moods/derive.py`, ohne Neugenerierung. Originale, Prompt und Ableitungen:
  [`design/brand/README.md`](design/brand/README.md).
- `design/brand/source/mochi-cloud.png` ist die ausgewählte KI-generierte Konzepttafel;
  Prompt, Generierungsweg und Modellnachweisgrenzen stehen daneben in `README.md`.
- Zen Maru Gothic von Yoshimichi Ohira / The Zen Maru Gothic Authors ist in
  Regular und Bold unverändert eingebettet (`assets/fonts/zen-maru-gothic/`).
  Lizenz: [SIL Open Font License 1.1](assets/fonts/zen-maru-gothic/OFL.txt).
  Feste Quelle und Dateihashes: [SOURCE.json](assets/fonts/zen-maru-gothic/SOURCE.json).
  Der native Build legt die vollständige Fontlizenz als `FONT-LICENSE.txt` bei.
  Der handgeschriebene mogumogu-Schriftzug ist Rasterkunst, keine Fontdatei.
- `docs/screenshots/` zeigen die native Anwendung mit Demodaten bzw. diesem Repository.

Dokumentation der verwendeten APIs: `docs/QUELLEN.md`.
