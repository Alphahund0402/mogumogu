# Mochi: Messung der nativen Konturstärke

Stand: 18. Oktober 2026. Gemessen wurde der tatsächlich eingebundene
`assets/brand/mascots-v2.png`-Bogen, SHA-256
`a58212b3515793ce166ae9a6a82add08b01dc99c940569b059966182244eee1b`.

**Die vier Posen liegen nah beieinander, haben aber keine exakt gleiche
Strichstärke.** An vergleichbaren Kopf- und Ohrkonturen beträgt der größte
Abstand zwischen den Medianen **0,107 logische Pixel bzw. 6,6 %** relativ zur
dünnsten Pose. Innerhalb der einzelnen Handzeichnungen schwankt die Kontur
stärker als zwischen den typischen Strichstärken der vier Posen.

## Ergebnisse

Die Referenzmessung verwendet 400-%-Renderings und rechnet physische Pixel
durch vier zurück. Alle Posen haben denselben nativen Bildbereich von
104 × 104 logischen Pixeln. Die letzte Spalte beschreibt die räumliche Streuung
der mittleren 50 % der Messstellen, kein Konfidenzintervall.

| Pose | Typische Konturstärke (Median) | 25.–75. Perzentil | Messstellen |
|---|---:|---:|---:|
| Neutral | 1,69 px | 1,52–1,85 px | 43 |
| Fröhlich | 1,73 px | 1,64–1,90 px | 43 |
| Mit Ordner | 1,62 px | 1,42–1,74 px | 43 |
| Am Laptop | 1,71 px | 1,57–1,88 px | 43 |

Bei 100 % liegen die Mediane der direkt gerenderten Pixel bei **1,81 / 1,91 /
1,86 / 1,82 px** in derselben Reihenfolge. Dort beträgt der größte Abstand
0,097 px. Die Abweichung zur Referenz entsteht durch Rasterung und
Interpolation bei Strichen, die nur etwa zwei Bildschirmpixel breit sind.
Die Zwischenstufen 125, 150 und 200 % stehen in [results.json](results.json).

[Bilder und einblendbare Messlinien](profiles.html) ·
[alle 860 Einzelmessungen als CSV](samples.csv) ·
[numerische Ergebnisse, Kalibrierung und Quellhashes](results.json).

| Neutral, 100 % | Fröhlich, 100 % | Mit Ordner, 100 % | Am Laptop, 100 % |
|---|---|---|---|
| ![Neutral](neutral-100.png) | ![Fröhlich](happy-100.png) | ![Mit Ordner](folder-100.png) | ![Am Laptop](focused-100.png) |

## Messverfahren

1. [render.rs](render.rs) lädt das unveränderte, von Slint 1.18.1 kompilierte
   `Dashboard`. Es rendert alle acht Ansichten bei 900 × 560 und 1080 × 680
   logischen Pixeln, jeweils mit 100, 125, 150, 200 und 400 % Skalierung.
   Die Skalierung erfolgt über Slints `ScaleFactorChanged`, anschließend wird
   die physische Fenstergröße gesetzt. Es werden keine Benutzerdaten geöffnet.
2. Aus jedem RGB-Puffer wird der native 104-Pixel-Bildbereich über Einstellungen
   unverändert ausgelesen. **80 von 80 Vergleichen bestätigen pixelgleiche
   Posen** über ihre zugehörigen Ansichten und beide Fenstergrößen bei derselben
   DPI-Stufe. Acht 100-%-Ausschnitte stimmen zusätzlich bytegleich mit den
   zuvor geprüften nativen Demo-Frames überein.
3. [measure.py](measure.py) wählt fünf korrespondierende Bereiche an Stirn,
   Ohren und oberen Kopfseiten aus. Pro Bereich gibt es neun vorab festgelegte
   Winkel. Zwei Stirnpositionen sind bei der Laptop-Pose wegen einer Lücke in
   der gezeichneten Kontur nicht auswertbar; dieselben Positionen werden bei
   allen vier Posen ausgelassen. Es bleiben je 43 vergleichbare Positionen.
   Die Auswahl und ausgelassenen Positionen sind im Skript bzw. JSON enthalten.
4. Im 400-%-Rendering wird die äußere Kontur entlang dieser Winkel gesucht.
   Nachbarpunkte bei ±2° liefern die lokale Tangente. Quer dazu wird das
   Helligkeitsprofil bilinear abgetastet: `0,2126 R + 0,7152 G + 0,0722 B` aus
   den sRGB-Kanalwerten. Gemessen wird die **Halbwertsbreite (FWHM)** zwischen
   lokalem Papier-/Fellniveau und dunklem Konturmaximum. Die feinere
   Abtastschrittweite von 0,025 logischen Pixeln erhöht nicht die Bildauflösung.
5. Dieselben Positionen und Normalen werden bei jeder DPI-Stufe ausgewertet.
   Bei den 860 Profilen gibt es keine weiteren Ausfälle. Die PNG-Belege sind
   verlustfreie Kodierungen der Renderer-Pixel, ohne Retusche oder Nachschärfung.

Zur Kalibrierung rendert derselbe native Renderer vier einfarbige Linien mit
bekannter Breite von 0,75 / 1,00 / 1,50 / 2,00 logischen Pixeln. Bei 400 % liefert
das Verfahren diese Werte auf 0,01 px gerundet exakt zurück. Bei niedriger DPI
werden sehr dünne Linien durch die Rasterung breiter erfasst; deshalb dient
400 % als Vergleichsreferenz. Diese Kalibrierung ersetzt keine Unsicherheitsanalyse
für die texturierten und gekrümmten Handzeichnungen.

## Einordnung und Umfang

Gemessen werden vergleichbare Außenkonturen der vier Ausdruckszeichnungen.
Augen, Mund, Wangenhatching, Schnurrhaare, Körperüberschneidungen, Ordner und
Laptop haben unterschiedliche Aufgaben und Geometrien und gehen nicht in
diesen Vergleich ein. Das ausgewählte Original-Kopficon wird separat mit
26 px Höhe angezeigt; die Wortmarke mit 112 × 25 px. Beide bleiben erhalten.

Die Messung belegt ähnliche typische Konturstärken und eine identische
Darstellung derselben Pose. Sie belegt keine konstante Breite jedes einzelnen
Strichs. Für die beobachteten 6,6 % wird keine allgemeingültige Wahrnehmungs-
oder Gleichheitstoleranz behauptet.

Die Aufnahmen stammen aus dem **nativen Slint-Software-Renderer**, ohne
OS-Fensterrahmen. Eine interaktive Windows-Abnahme von Monitorwechsel,
DWM-Skalierung, Tray und Eingabe bleibt in
[WINDOWS-ABNAHME.md](../../../docs/WINDOWS-ABNAHME.md) separat geführt.

## Erneut ausführen

Voraussetzungen: Windows, die Rust-Toolchain des Projekts, vorhandener
Cargo-Cache und ein eingerichtetes Python 3.11+; zusätzliche Python-Pakete
werden nicht benötigt. Vom Repository-Stamm:

```powershell
.\design\brand\measurement\render.ps1
python design/brand/measurement/measure.py
```

Der erste Schritt baut offline den nativen Debug-Stand und erzeugt die Puffer
unter `target/stroke-qa`. Der zweite aktualisiert die Messwerte und Belege in
diesem Verzeichnis. Quellhashes sind in `results.json` festgehalten.
`render.rs` setzt die aktuelle Sidebar-Geometrie voraus; bei Layoutänderungen
müssen die Bildkoordinaten erneut geprüft werden. Die Werkzeuge und Berichte
sind separate Designartefakte und werden nicht in die Anwendung eingebettet.
