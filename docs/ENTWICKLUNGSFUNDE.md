# Entwicklungsfunde, Speicher und Skill-Inhalte

Stand: 20. Oktober 2026. Die neuen Fähigkeiten sind experimentell. Tests verwenden
synthetische Windows-Verzeichnisse; eine Windows-11-Abnahme steht aus.

## Fundliste

Die Dateinamensuche (`projects`) und freigegebene Projektbereiche erkennen
Projektmarker, mögliche Scratchpads, einzelne Skripte ohne Projektmarker, Notebooks, Python-Umgebungen, Abhängigkeitsordner, typische
Build-/Testausgaben, Testdaten, lokale Datenbanken, bekannte Cache-Namen und
Ordner mit `CACHEDIR.TAG`, deren Inhalt nicht betreten wird.
Diese Erkennung liest nur Verzeichniseinträge. Große erkannte Speicher und
Ausgabeordner werden als Funde erfasst und in der Suche nicht weiter betreten.
Der bestehende Projektadapter kann nach Projektfreigabe eng begrenzte
Manifest-/Installationsmetadaten lesen.

Jeder Fund zeigt Typ, Beleg, Pfad, Scanabdeckung, mögliche Projektzuordnung und
bekannte Dateigröße/Änderungszeit. Ein Änderungsdatum ist kein Nutzungsnachweis.
Ordnernamen belegen weder Testzweck noch Entbehrlichkeit. Unbekannte Inhalte
bleiben unbekannt; Editor-Backups werden nicht als Scratchpads übernommen.

### Projekte

Ein Projekt erkennt mogumogu an Manifesten (`Cargo.toml`, `package.json`,
`pyproject.toml`, `go.mod`, `pom.xml`, Gradle, `.csproj`/`.sln`/`.vcxproj`,
`composer.json`, `Gemfile`, `mix.exs`, `pubspec.yaml`, `deno.json`,
`Package.swift`, `build.zig`, `build.sbt`, `stack.yaml`/`*.cabal`,
`meson.build`, `project.godot`, `*.uproject`, `flake.nix`), an `.git`/`.hg`/`.svn`
und an schwächeren Markern (`requirements.txt`, `setup.cfg`, `Pipfile`,
`Makefile`, `CMakeLists.txt`, `environment.yml`, `Dockerfile`, Compose-Dateien,
`*.code-workspace`, `*.Rproj`). Jeder Projektfund trägt eine Sicherheit:

| Sicherheit | Namen im Projektordner |
|---|---|
| hoch | Versionsverwaltung und Manifest, Versionsverwaltung mit eigener CI (etwa CMake- oder Make-Projekte), oder Manifest mit Lockfile oder Quellordner |
| mittel | nur Manifest, oder Versionsverwaltung ohne Manifest und CI |
| gering | nur schwache Marker; außerdem jeder Ordner mit dem Namen eines Archiv-Downloads ohne Versionsverwaltung (`*-main`, `*-master`, `name-1.4.2`) |

Liegt ein Projekt unter `Downloads`, ist die Sicherheit höchstens mittel. Bei
gleicher Sicherheit stehen Teilprojekte nach den äußeren Projekten. Daneben
nennt der Fund Hinweise: Versionsverwaltung, Lockfile, Quellcode, Tests, CI
(`.github`, `.gitlab-ci.yml` …), Editor-Einstellungen und KI-Dateien
(`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.claude`, `.cursor`, `.codex` …). Ein
Projekt in einem anderen Projekt derselben Suche ist ein **Teilprojekt**; sein
Projektbezug zeigt auf das äußere Projekt, und der Filter **Projekte** zeigt nur
die äußeren. Mitgelieferter Fremdcode (`vendor`, `third_party`,
`bower_components`, `Pods` …) ist in einem Projekt ein Abhängigkeitsfund und
wird nicht betreten. Als Aktivität zeigt der Fund das neueste Änderungsdatum der
Einträge des Ordners, auf den Tag gerundet. Sicherheit und Datum ordnen die
Liste; sie belegen weder Nutzung noch Entbehrlichkeit.

### Scratchpads

Ein Ordner gilt als mögliches Scratchpad, wenn sein Name auf einen Versuch
deutet und Entwicklungshinweise (Quellcode, `.venv`, `node_modules`,
`__pycache__`, Notebook-Checkpoints) oder Datendateien (`.csv`, `.parquet`,
`.jsonl`, `.npy`, `.pkl` …) vorliegen. Solche Namen sind:

- Wörter für Versuche: `scratch`, `scratchpad`, `scratches`, `experiment`,
  `playground`, `sandbox`, `spike`, `poc`, `prototype`, `tryout`, `throwaway`,
  `lab`, `versuch`, `spielwiese`, jeweils auch im Plural. `parser-spike` und
  `scratch_2024` zählen, `inspiker` nicht.
- Standardnamen von Projektvorlagen: `ConsoleApp3`, `WindowsFormsApp1`,
  `WebApplication2`, `pythonProject`, `untitled1`, `MyApplication`,
  `vite-project`, `my-app`, `New Unity Project`, `Neuer Ordner (2)` sowie
  `test2`, `tmp1`, `project4` (diese nur mit Nummer).
- Namen von Kopien: `… - Kopie`, `… - Copy`, `Copy of …`, `…_old`, `…-alt`,
  `….bak`, `…_backup`.

Außerdem gilt ein Ordner ohne Projektmarker außerhalb eines Projekts mit
mindestens zwei losen Skripten oder einem Notebook als mögliches Scratchpad. Der
Zweck bleibt unbekannt.

Bei einer Laufwerkssuche erscheinen einzelne lose Skripte nur dort, wo Dateien
abgelegt werden (Profilordner, Desktop, Downloads, Dokumente, auch in OneDrive),
oder wenn ihr Name nach Wegwerfdatei aussieht (`test_*.py`, `tmp.js`,
`untitled-1.ps1`, `foo.py`, `versuch.py` …).

**Bekannte Ablageorte von Werkzeugen.** Der Systembefund meldet den
Arbeitsordner von Claude Code (`%TEMP%\claude`), die JetBrains-Scratches
(`%APPDATA%\JetBrains`) und den temporären Ordner (`%TEMP%`). Keiner davon ist
Teil einer Laufwerkssuche, die AppData auslässt. Unter **Projekte → Funde →
Scratchpads & Skripte** erlaubt **Lesen erlauben** je Ort eine eigene
Namenssuche.

### Liste

Unter **Projekte → Funde** lassen sich Funde nach Projekten, Scratchpads/Skripten,
Umgebungen/Builds, Testdaten/Datenbanken und fehlendem Projektbezug filtern.
Projekte (nach Sicherheit) und Scratchpads stehen vor Notebooks, Skripten,
Testdaten und Datenbanken, danach folgen Umgebungen, Ausgaben und Caches.
Dieselbe Liste zeigt auch die bei der Projekterfassung erkannten Umgebungen,
Build-Ausgaben und Caches. Die Ansicht ist auf 200 Funde begrenzt. Die globale Suche
fragt gespeicherte Fundmetadaten unabhängig von dieser Grenze ab und öffnet auch
Funde außerhalb der ersten Seite. Für große Bestände die Suchwurzeln sinnvoll
aufteilen. Teilläufe ersetzen den letzten vollständigen Stand nicht.

Eine Zuordnung über **Projekt zuordnen** ist ausdrücklich, speichert einen
optionalen Zweck und lässt den Schutz aktiv. Sie gewährt keine zusätzlichen
Leserechte. Auch einzelne Dateien können so verfolgt werden. Projektregistrierung
ist nur für Verzeichnisse möglich. Veränderte oder redigierte Pfadmetadaten
dürfen nicht automatisch zu einem echten Dateipfad rekonstruiert werden.

Die Generationen vergleichen bei diesen Funden auch Dateigröße und
Änderungsdatum. Damit werden Veränderungen sichtbar, aber kein verursachender
Prozess und keine tatsächliche Verwendung bewiesen. Zweck und Besitzer eines
Experiments hält erst die ausdrückliche Zuordnung fest.

## Laufwerkssuche

**Speicher → Projekte suchen** erteilt mit einem Klick eine Namenssuche für eine
ganze lokale Festplatte (`D:\`) und startet sie; ein weiterer Klick sucht erneut.
Die CLI erreicht dasselbe mit `system approve --kind projects --path D:\`. Nur
diese Namenssuche darf eine Laufwerkswurzel freigeben; Projekt-, Paket- und
KI-Freigaben verlangen weiter einen Ordner. Netzlaufwerke, Wechselmedien und
optische Laufwerke werden abgelehnt. Die Freigabe ist an die Identität der
Laufwerkswurzel gebunden: Nach einer Neuformatierung ist sie blockiert, und
**Projekte suchen** erteilt sie bewusst neu.

Ausgelassen werden direkt unter der Wurzel `Windows`, `Windows.old`,
`Program Files`, `Program Files (x86)`, `ProgramData`, `Recovery`, `PerfLogs`,
`Boot`, `EFI`, `Config.Msi`, `MSOCache`, `OneDriveTemp` und die
`$…`-Update-/Wiederherstellungsordner; unter `Users` jedes Profil außer dem
eigenen (auch `Default`, `Public` und fremde Profile, selbst wenn der Zugriff
erlaubt wäre); im eigenen Profil `AppData`, `scoop`, `go\pkg` und alle
Punktordner (`.cargo`, `.rustup`, `.vscode`, `.claude` …); überall Spiele- und
App-Bibliotheken (`SteamLibrary`, `steamapps`, `Epic Games`, `GOG Games`,
`XboxGames`, `WindowsApps`, `WpSystem`). Diese Auslassungen gelten auch, wenn
an der Laufwerks- oder Profilwurzel ein Projektmarker liegt (`C:\.git`, ein
Dotfiles-Repository im Profil). Programmordner, also Ordner mit `.exe` und
`.dll`, aber ohne Projektmarker, werden nur außerhalb erkannter Projekte
ausgelassen. Globale Bestände und persönliche
KI-Ordner haben eigene Freigaben unter **Pakete** und **KI-Konfiguration**.

Ordner ohne Zugriffsrecht oder Ordner, die während des Durchgangs verschwinden,
zählen als ausgelassen und nicht als Lesefehler; auf einem Laufwerk sind sie
erwartbar (fremde Profile, Systemordner) und würden sonst jeden Durchgang
teilweise machen. Die Notiz der Generation nennt die Zahl der ausgelassenen
System-, Programm- und Werkzeugordner und der Ordner ohne Zugriffsrecht.
Einzelne lose Skripte werden auf einem Laufwerk nicht gelistet, weil sie dort
meist Installations- und Werkzeugdateien sind; Notebooks und mögliche
Scratchpads bleiben.

Ein Durchgang hat ein eigenes Budget: 2 Mio. Einträge, 32 Ebenen, drei Minuten
und null gelesene Dateibytes. Typische Entwicklungsorte (`Users`, `dev`, `src`,
`source`, `repos`, `projects`, `Desktop`, `Documents` …) werden zuerst besucht.
Reicht das Budget nicht, bleibt der Durchgang teilweise: Der letzte vollständige
Stand bleibt sichtbar, ein Laufwerk ohne vollständigen Stand zeigt noch keine
Funde. Die Laufwerkskarte nennt Funde und Zeitpunkt der letzten Suche oder
„Letzte Suche unvollständig“. Laufwerksfreigaben werden nicht überwacht, nicht
ausgemessen und nur auf Klick, über **Jetzt erfassen** oder `scan` erneut
durchsucht.

Diese Erweiterung der Lesegrenze braucht nach `AGENTS.md` einen zweiten Review;
er steht aus.

## Aufräumbericht

**Speicher → Aufräumen** (CLI `tidy [--project ID]`) misst auf Anfrage jedes
Projekt mit Lesefreigabe in einem begrenzten Durchgang ab der
identitätsgeprüften Wurzel: handle-relativ, ohne Links und Cloud-Platzhalter,
höchstens 2 Mio. Einträge und 120 Sekunden, ohne eine Datei zu öffnen. Jeder
Ordner wird mit seinen Nachbardateien gelesen
([Regeln](../src/inventory/tidy.rs)):

| Klasse | Ordner oder Datei | Beleg |
|---|---|---|
| Wiederherstellbar | `node_modules` | npm-, Yarn-, pnpm- oder Bun-Lockfile daneben |
| Wiederherstellbar | `.venv`, `venv` | `uv.lock`, `poetry.lock`, `Pipfile.lock` oder `pdm.lock` daneben **und** `pyvenv.cfg` darin |
| Wiederherstellbar | `target` | `Cargo.toml` daneben und `CACHEDIR.TAG`, `.rustc_info.json`, `debug` oder `release` darin; oder `pom.xml` daneben und `maven-status`, `maven-archiver` oder `classes` darin |
| Wiederherstellbar | `bin`, `obj` | .NET-Projekt oder -Lösung daneben; `bin` zusätzlich mit `obj` daneben und `Debug` oder `Release` darin, `obj` mit `project.assets.json` darin (sonst bleibt `bin` unberührt) |
| Wiederherstellbar | `build` | Gradle-Datei daneben und `tmp`, `classes`, `libs` … darin; `CMakeLists.txt` daneben und `CMakeCache.txt` darin; `setup.py`/`pyproject.toml` daneben und `lib.*`, `bdist.*` oder `temp.*` darin (ein bloßes `lib` reicht nicht). Neben `package.json` nie: dort liegen oft selbst gepflegte Dateien (electron-builder, alte Vue-Vorlagen) |
| Wiederherstellbar | `dist`, `out`, `.next`, `.nuxt`, `.svelte-kit`, `.output` | Build-Datei daneben (`package.json`, `pyproject.toml`, Gradle, CMake …) |
| Wiederherstellbar | `.tox`, `.nox` | `tox.ini` oder `noxfile.py` daneben |
| Wiederherstellbar | `__pycache__`; `.pytest_cache`, `.mypy_cache`, `.ruff_cache` mit `CACHEDIR.TAG`; `.gradle`, `.turbo`, `.parcel-cache`, `.angular` mit der Spur ihres Werkzeugs | Das Werkzeug legt sie beim nächsten Lauf neu an |
| Wiederherstellbar | `coverage` mit `lcov.info`, `coverage-final.json`, `clover.xml` …; `htmlcov` mit `status.json`; `test-results` mit `.last-run.json`; `test-output` mit `testng-results.xml`; `playwright-report` mit `data`; `.nyc_output` mit `processinfo` | Der nächste Testlauf legt sie neu an |
| Prüfen | Dieselben Ordner ohne ihren Beleg oder ohne die Spur darin; `.cache`; `build` neben `package.json` | Name allein kann auch Handarbeit sein (`coverage/` als Python-Paket) |
| Prüfen (Testdaten) | `testdata`, `test-data`, `test_data`, `testfiles`, `fixtures`, `__fixtures__`, `__snapshots__`; `data`, `resources`, `samples`, `assets` direkt in `test`, `tests`, `spec` oder `__tests__` | Kann selbst gepflegt sein; eigene Summe, „Viele Testdaten“ ab 1 GB oder ab 100 MB bei mindestens der Hälfte des eigenen Projektinhalts |
| Prüfen | `logs`, Datenbankdateien, Logs ab 10 MB, jede Datei ab 100 MB | Zweck unbekannt |

Alles andere zählt zum Projekt; `.git`, `.hg` und `.svn` erscheinen getrennt
als Versionsverwaltung. Als letzte Änderung gilt die neueste Datei außerhalb
von Ausgaben, Caches und Versionsverwaltung. Ein Projekt, das seit über 180
Tagen unverändert ist und Wiederherstellbares enthält, trägt „Ruht“. Name und
Alter allein machen nichts wiederherstellbar.

Freigegebene globale Bestände werden als Ganzes gemessen: der NuGet-
Paketspeicher als wiederherstellbar (Pakete werden neu geladen), installierte
npm-, uv-, Cargo- und Python-Bestände als „Prüfen“. Paket-Caches haben eigene
Freigaben (Systembefund unter **Pakete → Werkzeuge & globale Bestände** oder
CLI `system approve --kind npm-cache` …): npm-, pip- und uv-Cache,
Cargo-Registry, Gradle-Caches, pnpm-Store und Yarn-Cache gelten als
wiederherstellbar und tragen den Aufräumbefehl ihres Werkzeugs (etwa
`npm cache clean --force`, `uv cache prune`, `pnpm store prune`); das
Maven-Repository bleibt „Prüfen“, weil es mit `mvn install` selbst gebaute
Artefakte enthalten kann. Ein Cache wird nur gemessen, nie als Paketbestand
gelistet, und mogumogu führt keinen Aufräumbefehl aus. Projekte ohne Lesefreigabe
werden gezählt, aber nicht gemessen. Der Bericht bleibt im Speicher des
Besitzerprozesses, bis er neu erstellt wird oder mogumogu endet; nur die
gemessene Projektgröße wird gespeichert. Er entfernt nichts. Eine Freigabe
zu widerrufen nimmt das Projekt aus dem Bericht.

**Alle übernehmen** (CLI `scratch adopt-found`) erfasst jeden aktuellen
Scratchpad-Fund, den noch keine Ressource verfolgt, als geschützte Ressource
mit Prüftermin in 30 Tagen. Liegt der Fund in einem registrierten Projekt, gilt
dieses als vermuteter Besitzer. Geänderte oder redigierte Pfade werden
übersprungen; Lesefreigaben entstehen nicht.

**Alle übernehmen …** unter **Funde → Projekte** (CLI `project accept-all`)
zeigt zuerst alle sicheren äußeren Projekte vollständiger Projektsuchen: Fund
mit Sicherheit „hoch“, genauer Pfad, kein Projekt darüber und keines, das ihn
schon enthält. Erst die Bestätigung, gebunden an den Fingerabdruck dieser Liste,
registriert jedes Projekt und erteilt seine identitätsgebundene Lesefreigabe;
danach folgt eine Erfassung. Ein Ordner, der sich nicht freigeben lässt (etwa
inzwischen entfernt), wird mit Grund ausgelassen und nicht registriert.
Teilprojekte, unsichere und redigierte Funde bleiben Einzelentscheidungen.

## Laufwerke

**Speicher** zeigt Windows-Laufwerke, OS-Kapazität, belegten/freien Speicher,
registrierte Projekte und gemessene Anteile an der Laufwerkskapazität. Größen
werden auf Anfrage gemessen. Normale Snapshot-Aktualisierungen führen keinen
Dateibaumscan aus; OS-Metadaten werden höchstens 30 Sekunden zwischengespeichert.
**Aktualisieren** fragt die Kapazität erneut ab. Auf Wechselmedien, Netzlaufwerken und optischen Laufwerken bleibt
die Kapazität ohne Abfrage unbekannt.

Projektgrößen sind logische Dateigrößen, keine physisch freigebbare Belegung.
Hardlinks, Kompression und gemeinsame Daten können abweichen. Verschachtelte
Projekte und identische normalisierte Projektpfade werden in der Laufwerkssumme
nicht doppelt addiert. Unvermessene Projekte machen die Gesamtsumme unbekannt.
Gespeicherte Messungen werden als früherer Messstand gekennzeichnet
und können inzwischen verändert sein. Die Gruppierung verwendet den registrierten
Laufwerksbuchstaben; Mountpoints/SUBST werden nicht physisch aufgelöst.

Einen Ordnerbrowser gibt es nicht mehr (entfernt am 24. Oktober 2026): Was ein
Projekt belegt und was davon wiederherstellbar ist, zeigt der Aufräumbericht.

## Skill-Inhalte

In **KI-Konfiguration** öffnet ein erkannter Skill, eine Instruktion oder Regel
über **Inhalt ansehen** einen schreibgeschützten, scrollbar auswählbaren Text.
Jedes Öffnen prüft veröffentlichte Fundmetadaten, Katalog, aktuelle Freigabe und
Ordneridentität erneut. Es werden keine Verweise verfolgt und keine gefundenen
Anweisungen ausgeführt. Gemischte Konfigurationsdateien und Credentials sind
keine Inhaltsquellen dieses Viewers.

Texte bleiben flüchtige Anzeigedaten und gelangen nicht in SQLite, Snapshot,
Aktivität oder Export. Bekannte sensible Muster werden redigiert; das ist keine
vollständige semantische Geheimniserkennung. Die Größe ist auf höchstens 128 KiB
und zusätzlich das jeweilige Profillimit begrenzt. Binäre, zu große oder
ungültige UTF-8-Dateien werden abgewiesen. Die Demo verwendet nur einen
eingebauten Beispieltext und synthetische Laufwerksdaten.

## CLI

```powershell
mogumogu-cli discoveries
mogumogu-cli project accept-all                     # Vorschau mit Fingerabdruck
mogumogu-cli project accept-all --confirm FINGERABDRUCK
mogumogu-cli resource adopt --scope 2 --relative experiments/probe.py --project 1 --purpose "Parserprobe"
mogumogu-cli storage
mogumogu-cli ai content --scope 1 --relative .agents/skills/beispiel/SKILL.md
```

## Prüfnachweise und Grenzen

`tests/package_scopes.rs` prüft Erkennung, Kontext, Zuordnung, Schutz,
Idempotenz, Teilläufe und den Zugriff außerhalb des Seitenlimits.
`tests/disk.rs` prüft Freigaben, Austausch, Widerruf, Junctions, Limits,
Summierung und Demotrennung. `tests/ai_content.rs` prüft flüchtige Inhalte,
Redaktion, Austausch, Widerruf, Pfadausbruch und Format-/Größenlimits.
Die Leseerweiterungen wurden zusätzlich von einem getrennten Agenten geprüft.
Dies ersetzt keine Windows-11- oder menschliche Sicherheitsabnahme.

Quellen: [Windows-Laufwerke](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getlogicaldrives),
[Laufwerkstyp](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew),
[Kapazität](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdiskfreespaceexw),
[Slint TextEdit](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/textedit/).
