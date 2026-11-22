# Schwachstellen-Watcher

Der optionale Watcher gleicht exakte Paketversionen mit einer lokalen Kopie
öffentlicher OSV-Advisories ab. Im Projektmodus zeigt er Befunde für alle
registrierten Projekte oder ein einzelnes Projekt. Der lokale HTML-Bericht
verlinkt OSV-Advisories und vorhandene CVE-IDs. Die aktuelle vollständige
Generation einer freigegebenen Quelle liefert die Paketbelege. Fehlende
Freigaben und unvollständige Erfassungen sind im Bericht sichtbar.

## Abdeckung

| Quelle | Überprüfte Identität |
|---|---|
| npm, auch globale CLI-Pakete | Installiert laut verstecktem npm-Lockfile in `node_modules` oder den Metadaten des freigegebenen globalen Bestands |
| Python und uv-Tools | Installiert laut `.dist-info` in freigegebenen Umgebungen. Ohne Herkunftsbeleg erscheint ein Treffer als möglicher Namens- und Versionsabgleich mit PyPI |
| Cargo-Tools | Name und Version des installierten Crates laut Installationsdatensatz |
| Eigene Werkzeugliste | Exakte Identitäten aus den unterstützten OSV-Ökosystemen sowie Go `stdlib` und `toolchain` |
| Projektmodus | Aufgelöste und installierte Versionen aus Projektinventaren für npm, PyPI, crates.io, Go, NuGet, Maven, Packagist (Composer), RubyGems (Bundler), Pub (Dart/Flutter), Hex (Mix), CRAN (R/renv ohne Bioconductor) und Julia |

Der Bericht zeigt Auflösung und Installation als eigene Belege. Ein Lockfile
belegt die dort dokumentierte Version. Go-Prüfsummen können frühere
Modulversionen enthalten. Bei mehreren Zielplattformen oder Frameworks
berücksichtigt der Watcher die aufgelösten Versionen aus allen erfassten
Varianten. Die Herkunft bleibt bei vielen Formaten offen und Treffer dafür
gelten als mögliche Zuordnung. Bekannte private Quellen, Git- und Pfadpakete
sowie Pakete ohne unterstützte Identität zählen als übersprungene Belege.
Der Abgleich deckt bekannte Paket-Advisories ab. Für eigenen Quellcode,
Container-Images und eingebettete Bibliotheken braucht es weitere Belege.
Deklarierte Versionsbereiche und reine Cache-, PATH- oder Shim-Funde liefern
keine exakte Version für den Vergleich. [OSV-Schema mit CVE-Aliasen](https://ossf.github.io/osv-schema/#aliases-field)

## Start unter Windows

1. Baue die beiden mogumogu-Programme aus diesem Quellstand. Starte den Besitzer
   im Lokalmodus. Gib nur die tatsächlich benötigten Bestände zum Lesen frei
   und erfasse sie im Dashboard oder per `scan --scope ID`.
2. Lege OSV-Scanner v2.7.1 außerhalb des Projekts ab und vergleiche SHA-256 mit
   den Release-Angaben. Der Watcher wurde mit dieser Version und synthetischen
   lokalen Datenbanken geprüft. [Offizielles Release](https://github.com/google/osv-scanner/releases/tag/v2.7.1)
3. Führe den Watcher mit Python 3.11 oder neuer aus. Das Beispiel setzt voraus,
   dass der Build unter `dist\mogumogu` liegt.

```powershell
$cli = (Resolve-Path .\dist\mogumogu\mogumogu-cli.exe).Path
$scanner = 'C:\Tools\osv-scanner_windows_amd64.exe'
$cache = Join-Path $env:LOCALAPPDATA 'mogumogu-advisories'

python scripts/watch_advisories.py refresh --cache $cache
python scripts/watch_advisories.py check --cache $cache --cli $cli --scanner $scanner
python scripts/watch_advisories.py watch --cache $cache --cli $cli --scanner $scanner --refresh
```

### Projekte prüfen und den Bericht öffnen

```powershell
$report = Join-Path $cache 'projekte.html'
python scripts/watch_advisories.py refresh --projects --cache $cache
python scripts/watch_advisories.py check --projects --scan --cache $cache --cli $cli --scanner $scanner --html $report
Invoke-Item -LiteralPath $report
```

`--projects` wertet alle registrierten Projekte aus. `--project ID` wählt ein
Projekt anhand der im Bericht angezeigten ID. `check --scan` startet vor der
Auswertung eine statische Erfassung der vorhandenen Projektfreigaben. Wenn du
die Erfassung im Dashboard abbrichst, verwendet der Bericht die verfügbaren
Stände und zeigt den Abbruch an. Der Abgleich ohne `--scan` liest veröffentlichte
Inventardaten. Der Bericht zeigt je Projekt die Zahl abgeglichener
Versionen, den Erfassungsstand und die Befunde. Die Auswertung liest bis zu
20.000 Installations- und Auflösungsbelege. Bei erreichter Grenze kannst du
ein einzelnes Projekt gezielt prüfen.

```powershell
python scripts/watch_advisories.py watch --projects --cache $cache --cli $cli --scanner $scanner --refresh --html $report
```

`watch` prüft direkt nach dem Start und danach alle sechs Stunden
(`--interval-minutes` ändert den Abstand). Er schreibt beim Start und bei einer
Änderung einen JSON-Bericht auf stdout und aktualisiert die angegebene
HTML-Datei. `Strg+C` beendet ihn. Eine abweichende
lokale Datenbank gibst du mit `--data-dir` an. Neue oder aktualisierte
Installationen werden nach der nächsten expliziten Inventarerfassung geprüft.
Der Watcher verwendet den vorhandenen Besitzerprozess. Tray, Autostart und
Aufgabenplanung sind unabhängig vom Watcher.

Der Befehl `refresh` und der Schalter `--refresh` laden die öffentlichen Archive
unabhängig vom Inventar. Der Projektmodus ergänzt NuGet, Maven, Packagist,
RubyGems, Pub, Hex, CRAN und Julia zu npm, PyPI, crates.io und Go. Mit dem Schalter wird ein Download frühestens
bei einem Tag Datenalter fällig. Ein erfolgreicher Download ersetzt die alte
ZIP-Datei nach Integritätsprüfung. Für eine durchgehend offline betriebene
Instanz können die ZIP-Dateien von einem anderen Rechner in den Cache
kopiert werden. Der Scanner erhält nur die Bezeichner der überprüfbaren
Versionen in einer temporären lokalen Datei. Projektpfade und Registry-Hosts
bleiben im Inventar. [OSV-Datenexporte](https://google.github.io/osv.dev/data/)
[Offline-Modus und Cache-Struktur](https://google.github.io/osv-scanner/usage/offline-mode/)

Für ein Werkzeug mit selbst verifizierter Version lege eine lokale JSON-Datei
außerhalb der Arbeitskopie an und gib sie mit `--tools PFAD` an. Diese Belege
erscheinen als `user_reported_tool`. Die Liste zeigt eine selbst geprüfte Go-Version.

```json
[
  {"ecosystem": "Go", "name": "stdlib", "version": "1.25.6"},
  {"ecosystem": "Go", "name": "toolchain", "version": "1.25.6"}
]
```

## Befunde prüfen

- Der Bericht nennt geprüfte Versionen als Anzahl, Quellenstatus und Treffer mit
  Hinweisen auf die lokalen Freigaben. `public_scope_ids` stützt sich auf den
  öffentlichen Quellenbeleg des Adapters. `possible_scope_ids` und
  `possible_packages` machen die Fälle ohne Herkunftsbeleg sichtbar.
  `cve_ids` enthält die CVE-Kennungen aus den Advisory-Aliasen, sofern vorhanden.
- `partial` kennzeichnet fehlende, unlesbare oder mehr als sieben Tage alte
  Eingaben, übersprungene Einträge und Auswertungslimits. Treffer aus älteren
  verfügbaren Daten bleiben sichtbar. Ein leerer Befund gilt nur für die
  geprüften Identitäten und den angegebenen Datenstand.
- `check` liefert Status `0` für einen vollständigen Vergleich seines
  unterstützten Umfangs ohne Treffer, `1` für Treffer bei vollständigem
  Vergleich und `2` bei Teilabdeckung oder einem Fehler. Auch bei Status `2`
  enthält der Bericht alle verfügbaren Treffer.
- Prüfe einen möglichen Treffer gegen die tatsächliche Paketquelle. Öffne
  danach den OSV-Hinweis und folge der passenden Herstellerempfehlung für die
  installierte Version. Aktualisiere über den verantwortlichen Paketmanager,
  teste die betroffenen Projekte und erfasse die neue Version erneut. Prüfe
  bei RustSec-Hinweisen auch den Typ, etwa Sicherheitslücke oder Wartungsstatus.

Bei automatisierten Prüfungen wird die Versionsübereinstimmung mit
synthetischen Installationsbelegen und Advisory-Daten getestet (in der isolierten
Docker-Sandbox `test-sandbox` auch direkt offline gegen den per SHA-256 geprüften
OSV-Scanner v2.7.1). Die lokalen Berichte können Paketnamen enthalten und gehören
in ein privates Verzeichnis.

## Sicherheits-Audit der eigenen `Cargo.lock`

Für mogumogus eigene Rust-Abhängigkeiten prüft [audit_cargo_lock.py](../scripts/audit_cargo_lock.py)
die `Cargo.lock` offline gegen das OSV-`crates.io`-Archiv und die befristete
Triage-Liste [advisory-triage.toml](../security/advisory-triage.toml). Neue
Advisories, Versionswechsel ohne Triage oder abgelaufene Fristen beenden den Lauf
mit Status `1`, ein fehlender oder über sieben Tage alter Feed mit Status `2`.
Reproduzierbar im isolierten Offline-Container:

```powershell
docker compose run --build --rm security-audit
```

Zusätzlich führt der Workflow [security-audit.yml](../.github/workflows/security-audit.yml)
den Audit bei Änderungen an `Cargo.lock` oder der Triage-Liste sowie wöchentlich
geplant aus. Details zur Container-Isolierung stehen in [DOCKER.md](DOCKER.md).

## Eigenständige Library & CLI (`mogumogu-advisories`)

Die komplette Prüf-, Lockfile-, Triage- und Berichtlogik liegt als eigenständiges,
abhängigkeitsfreies Python-Paket (Python 3.11+ Stdlib, `GPL-3.0-only`) unter
[packages/mogumogu-advisories/](../packages/mogumogu-advisories/README.md).
Sie lässt sich direkt in fremden Projekten und CI/CD-Pipelines einsetzen:

- **Offline-Installation:** `pip install --no-build-isolation ./packages/mogumogu-advisories`
- **Direktes Lockfile-Scannen:** liest 15 Lockfile-Formate der zwölf unterstützten
  OSV-Ökosysteme statisch und ohne Programmstarts (`Cargo.lock`, `package-lock.json`,
  `pnpm-lock.yaml`, `uv.lock`, `poetry.lock`, `requirements.txt`, `go.mod`,
  `packages.lock.json`, `gradle.lockfile`, `composer.lock`, `Gemfile.lock`,
  `pubspec.lock`, `mix.lock`, `renv.lock`, `Manifest.toml`).
- **Ausgabeformate:** **SARIF 2.1.0** (für GitHub Code Scanning / IDEs), **JSON**
  und eigenständiger **HTML-Bericht**; befristete Triage-Manifeste (TOML) und
  Datenalter-Prüfung (Fail-Closed).
- **Wiederverwendbare Pipeline-Bausteine:** eigenes [Dockerfile](../packages/mogumogu-advisories/Dockerfile)
  und wiederverwendbarer GitHub-Actions-Workflow
  [advisory-audit-reusable.yml](../.github/workflows/advisory-audit-reusable.yml).
- Die Skripte unter `scripts/` (`watch_advisories.py`, `audit_cargo_lock.py`,
  `advisory_report.py`) delegieren direkt an diese Library.
