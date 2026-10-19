# mogumogu — Designvarianten

**Ausgewählt:** A / Mochi aus der Manga-Runde. Icon, Maskottchen und
Schriftzug sowie die japanischfähige Anwendungsschrift sind in der
[verbindlichen Gestaltung](../brand/README.md) festgehalten.

Neue Runde: [KI-generierte Manga-Varianten](manga/index.html) mit Maskottchen,
Ausdrucksstudien und passenden Windows-Ansichten. Modell-/Generierungshinweise
und die vollständigen Prompts stehen [daneben](manga/README.md).

**Öffnen:** [index.html](index.html) doppelklicken. Alles funktioniert lokal und
ohne Installation oder Server. Mit A/B/C oder den Pfeiltasten vergleichen;
Logos lassen sich unabhängig vom Layout wählen. Die URL merkt sich die Auswahl,
zum Beispiel `index.html?variant=b&logo=a`.

| Variante | Aufbau | Logo |
|---|---|---|
| [A · Cloud](a-cloud.png) | Benannte Seitennavigation, Projekttabelle, Detailbereich; größte Nähe zur aktuellen App | [Mochi](logo-a-mochi.svg): rundes, neugieriges Gesicht |
| [B · Moss](b-moss.png) | Hauptbereiche als Tabs, Projektliste und ausgewählter Arbeitsbereich; warme Weiß-/Salbeitöne | [Nibble](logo-b-nibble.svg): zufriedenes Gesicht, kleine angehobene Pfote |
| [C · Midnight](c-midnight.png) | Schmale Iconleiste, Projekte, Prüfliste und Kontext; dunkle Oberflächen | [Pocket](logo-c-pocket.svg): Katze mit zwei Pfoten |

[Gesamtvergleich](comparison.png) · [Logos bei 16/24/32 px](logos.png).

Die Richtung A ist die Empfehlung für einen behutsamen Ausbau der vorhandenen
Windows-App. B legt den Schwerpunkt auf einzelne Projekte; die Tabs brauchen
bei schmalen Fenstern eine kompakte Darstellung. C priorisiert Prüfbelege; die
Iconleiste braucht bei einer Umsetzung Tooltips, zugängliche Namen und eine
Option zum Aufklappen der Beschriftungen. Layout und Logo können frei kombiniert
werden. Die Studien bleiben zum Vergleich archiviert.

## Kontext und Grenzen

Ziel ist ein kompaktes Werkzeug neben dem Editor, bedienbar mit Maus und
Tastatur. Diese Annahme folgt dem Projektumfang; es liegt keine Nutzerstudie
vor. Basis: Windows 10/11, 1080 × 680, vorhandene Segoe UI, native Slint-Laufzeit,
lokales Inventar, manuelle Lesefreigaben und beleggebundene Prüfhinweise.

Alle Ansichten sind **statische Vektorentwürfe mit synthetischen Daten**, keine
Screenshots eines laufenden Windows-Fensters. Die Galerie ist ein gesondertes,
schreibgeschütztes Designwerkzeug. Nur Layout und Logo des Entwurfs wechseln;
es gibt keine Verbindung zum Kern, zu echten Projekten oder zur Datenbank.
Neue Formulare, Navigation, DPI, Kontraste und Screenreader-Verhalten benötigen
nach der Auswahl eine eigene Umsetzung und Prüfung in Slint.

Die ursprüngliche Katzenform und die Linienicons wurden als Ausgangspunkt
verwendet. Alle drei neuen Logos sind editierbare SVGs mit transparentem
Hintergrund; die PNG-Dateien sind davon abgeleitet. Keine externen Bilder oder
Fontdateien sind eingebunden. Quellcode und eigene Entwürfe: GPL-3.0-only.

## Entwürfe neu erzeugen

```powershell
python design/variants/generate.py
.\design\variants\render.ps1
```

`generate.py` verwendet ausschließlich die Python-Standardbibliothek. Die
HTML-Vorlage ist `gallery.html`; `index.html` enthält anschließend alle
Vektoren selbst. `render.ps1` verwendet Rust und den bereits für Slint gebauten
SVG-Renderer aus `target/debug/deps`. Falls dessen Cache fehlt, zuerst einen
Debug-Build der nativen App ausführen. Der Vorschau-Renderer lädt nur vorhandene
Segoe-UI-Systemschriften in den Speicher; er kopiert keine Fontdateien.
Build-Ausgabe bleibt unter `target/`.

Nach der Auswahl wird die gewählte Gestaltung in die bestehenden Slint-Module
übertragen und das Logo für App-/Tray-Größen aufbereitet. Die Galerie bleibt
bis dahin eine Designstudie und wird nicht von `build.ps1` paketiert.
