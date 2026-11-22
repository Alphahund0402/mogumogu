# mogumogu-advisories

**Offline-first OSV/CVE Vulnerability-Audit-Library und CLI für Projekte und CI/CD-Pipelines**

- **Python 3.11+, Standardbibliothek pur:** null Laufzeit- und Build-Abhängigkeiten (`pip install --no-build-isolation` funktioniert komplett offline).
- **12 OSV-Ökosysteme & 15 Lockfile-Formate:** `crates.io` (`Cargo.lock`), `npm` (`package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml`), `PyPI` (`uv.lock`, `poetry.lock`, `requirements.txt`), `Go` (`go.mod`), `NuGet` (`packages.lock.json`), `Maven` (`gradle.lockfile`), `Packagist` (`composer.lock`), `RubyGems` (`Gemfile.lock`, `gems.locked`), `Pub` (`pubspec.lock`), `Hex` (`mix.lock`), `CRAN` (`renv.lock`, ohne Bioconductor) und `Julia` (`Manifest.toml`, `JuliaManifest.toml`).
- **Offline-Abgleich mit verifiziertem OSV-Scanner v2.7.1:** SHA-256-geprüfter Download, danach reine Offline-Auswertung (`--offline --no-call-analysis=all`) gegen lokale OSV-`all.zip`-Archive.
- **Befristete Triage (TOML) & Fail-Closed-Vertrag:** Neue Advisories oder abgelaufene Triage-Fristen beenden den Lauf mit Exit `1`; fehlende, defekte oder zu alte OSV-Feeds (> 7 Tage) sowie leere/fehlerhafte Scans beenden mit Exit `2`.
- **Pipeline-Formate:** **SARIF 2.1.0** (für GitHub Code Scanning / Security Tab / IDEs), **JSON** und eigenständiger, CSP-gehärteter **HTML-Bericht** mit OSV- und CVE-Links.

## Installation

Aus diesem Repository (auch komplett offline):

```powershell
pip install --no-build-isolation ./packages/mogumogu-advisories
```

Entwicklungsinstallation (editable):

```powershell
pip install --no-build-isolation -e ./packages/mogumogu-advisories
```

## CLI-Nutzung (lokal & in Pipelines)

Cache (`%LOCALAPPDATA%\mogumogu-advisories` bzw. `~/.cache/mogumogu-advisories` oder `MOGUMOGU_OSV_CACHE`),
Scanner (`<cache>/bin/osv-scanner` oder `MOGUMOGU_OSV_SCANNER`) und Triage-Datei
(`security/advisory-triage.toml` bzw. `advisory-triage.toml`) werden automatisch gefunden:

```powershell
# Erster Lauf / Aktualisierung (lädt SHA-256-geprüften Scanner + benötigte OSV-Feeds)
mogumogu-advisories audit . --update --out reports

# Danach jederzeit komplett offline (ohne Netzwerk, mit automatischer Triage-Erkennung)
mogumogu-advisories audit .
```

`--out reports` schreibt `audit.json`, `audit.html` und `audit.sarif` gesammelt in einen Ordner.
Mit `--no-triage` lässt sich eine automatisch gefundene Triage-Datei explizit ignorieren; alle
Pfade (`--cache`, `--scanner`, `--triage`, `--json`, `--html`, `--sarif`) bleiben weiterhin einzeln überschreibbar.

### Exit-Codes für CI/CD

| Code | Bedeutung |
|---|---|
| `0` | Vollständiger Audit, Feeds aktuell, keine ungetriagten oder abgelaufenen Advisories |
| `1` | Mindestens ein ungetriagtes Advisory oder eine abgelaufene Triage-Frist |
| `2` | Unvollständiger Audit (fehlender/defekter/veralteter Feed, keine prüfbaren Pakete, Parse- oder Scannerfehler) |

## Python-Library-API

```python
from pathlib import Path
from mogumogu_advisories import (
    audit_project,
    refresh_feeds,
    write_audit_html,
    write_sarif,
)

cache = Path("./osv-cache")
scanner = Path("./tools/osv-scanner")

# Optional vorab nur benötigte Ökosysteme aktualisieren
refresh_feeds(cache, ecosystems=["crates.io", "npm", "PyPI"])

exit_code, report = audit_project(
    Path("."),
    cache=cache,
    scanner=scanner,
    triage_path=Path("security/advisory-triage.toml"),
    max_age_days=7,
)

write_sarif(Path("reports/audit.sarif"), report)
write_audit_html(Path("reports/audit.html"), report)
```

## Befristetes Triage-Manifest (`advisory-triage.toml`)

```toml
[[advisory]]
id = "RUSTSEC-2026-0459"
ecosystem = "crates.io"   # optional bei Nutzung von `crate = "..."`
package = "libsqlite3-sys"
version = "0.38.2"
scope = "windows-build"
kind = "vulnerability"
expires = "2026-12-31"
reason = "Upgrade auf rusqlite 0.41.0 nach Migrationsabnahme geplant."
```

Jeder Eintrag gilt ausschließlich für die exakte Kombination aus `(id, ecosystem, package, version)` bis einschließlich `expires`.

## Eigenständiges Docker-Image & GitHub Actions

Das mitgelieferte `compose.yaml` kapselt alle Container-Härteflags (`network_mode: none`,
`read_only: true`, `cap_drop: [ALL]`, non-root `10001:10001`, Cache-Volume `osv-cache`):

```powershell
# 1. Benötigte OSV-Feeds für das aktuelle Projekt in das Cache-Volume laden
docker compose -f packages/mogumogu-advisories/compose.yaml run --rm refresh

# 2. Projekt komplett offline prüfen (inkl. automatischer Triage-Erkennung)
docker compose -f packages/mogumogu-advisories/compose.yaml run --rm audit

# Optional für ein anderes Projektverzeichnis:
$env:TARGET = "C:\dev\anderes-projekt"
docker compose -f packages/mogumogu-advisories/compose.yaml run --rm audit
```

Alternativ direkt per `docker run` (Image setzt `MOGUMOGU_OSV_CACHE=/cache`, `MOGUMOGU_OSV_SCANNER` und Standardbefehl `audit /work`):

```powershell
docker build -t mogumogu-advisories ./packages/mogumogu-advisories
docker run --rm -v osv-cache:/cache -v "${PWD}:/work:ro" mogumogu-advisories refresh --project /work
docker run --rm --network none --read-only --tmpfs /tmp -v osv-cache:/cache:ro -v "${PWD}:/work:ro" mogumogu-advisories
```

Für GitHub Actions steht zudem der wiederverwendbare Workflow `.github/workflows/advisory-audit-reusable.yml` (`workflow_call`) mit optionalem SARIF-Upload und HTML/JSON-Artefakten bereit.

## Sicherheits- und Abdeckungsgrenzen

- Statisches Lesen ohne Programmstarts: Kein Paketmanager, Interpreter, Build-Skript oder Hook wird ausgeführt. Symbolische Verknüpfungen werden beim Durchsuchen nicht verfolgt.
- Private Registries, Git-/Pfadabhängigkeiten, Bioconductor-Einträge und ungenaue Versionsbereiche (z. B. `>=` in `requirements.txt`, `dev-main` in Composer) werden bewusst übersprungen und im Feld `skipped` ausgewiesen.
- Ein Exit-Code `0` gilt ausschließlich für die exakt erfassten Lockfile-Versionen, die unterstützten Ökosysteme und das gemeldete Datenalter der OSV-Archive — er ist kein pauschaler Nachweis für die Gesamtsicherheit eines Systems oder eigenen Quellcodes.

## Lizenz

`GPL-3.0-only`, siehe `LICENSE`.
