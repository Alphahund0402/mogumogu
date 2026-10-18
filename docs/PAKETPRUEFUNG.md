# Paketbelege und Abkündigungshinweise

Stand: 17. Oktober 2026. Experimenteller, statischer Umfang für Windows 10/11.

## Was die Aussagen bedeuten

Die Ansicht **Pakete** trennt Version, Quelle, deklarierte/aufgelöste/installierte
Metadaten, mögliche Referenzen und Quellenstatus. Ein NuGet-Cacheeintrag bleibt
ein Cacheeintrag. Eine CLI kann ohne Projektbezug benötigt werden. Keine dieser
Aussagen erteilt eine Löschfreigabe.

Ein Projekt-Lockfile ist ein konkreter statischer Versionsbeleg. Eine Deklaration
ohne aufgelöste Version/Quelle, ein Elternpaket-Verweis, ein Befehlsname oder ein
MCP-Eintrag erscheint als **möglicher Verweis**. Das ist kein Ausführungs- oder
Nutzungsnachweis. Lockfiles können ebenfalls veraltet sein; nach einem partiellen
Lauf bleibt der frühere vollständige Stand ausdrücklich als älterer Stand sichtbar.

Abdeckung zählt vollständig erfasste registrierte Projekte, ungeprüfte/teilweise
erfasste Projekte und freigegebene Suchwurzeln. „Vollständig“ gilt innerhalb der
unterstützten Selektoren und Grenzen. Nicht registrierte Projekte, übersprungene
Links, dynamische Imports, bedingte Abhängigkeiten und manuelle CLI-Nutzung bleiben
unbekannt. Es gibt kein Urteil „in keinem Projekt verwendet“.

**Behalten** merkt die konkrete Paketidentität lokal vor. Die Entscheidung kann
gelöst werden und unterdrückt keine Upstream-Warnung. Sie startet keine Deinstallation.

## Globale Bestände und Projektkandidaten

Im Dashboard: **Pakete → Werkzeuge & globale Bestände → Lesen erlauben** für
gefundene Bestände, **Ort hinzufügen …** für Python-Umgebungen und abweichende
Orte, sowie **Projekte → Funde → Suchordner hinzufügen …** für die
Dateinamensuche. Die Freigabe ist an die Ordneridentität gebunden; anschließend
startet eine Erfassung.

| Art | Wurzel / erfasste Metadaten | Grenze |
|---|---|---|
| `projects` | Expliziter Suchordner; Namen von Projektmanifesten, auch verschachtelt | Keine Dateiinhalte; Git-Interna, Links, bekannte Umgebungs-/Ausgabeordner ausgelassen; Kandidaten separat registrieren und freigeben |
| `npm-global` | `npm_config_prefix\node_modules`, sonst `%APPDATA%\npm\node_modules`; Paket-`package.json`, auch Scoped-Pakete | Ohne `_resolved` keine bekannte Registry; keine `.npmrc`, Skripte oder Payload-Prüfung |
| `uv-tools` | `UV_TOOL_DIR`, sonst aktueller Standard `%APPDATA%\uv\data\tools`; Toolordner mit `uv-receipt.toml` und Python-Metadaten | Ältere/abweichende Orte über Pfad angeben; keine Interpreter, keine Cache-Erfassung |
| `python` | Explizite Interpreter-/venv-Wurzel mit `Lib\site-packages` oder direkt ein `site-packages`-Ordner | `.dist-info`, ausgewählte `METADATA`-Header, Anzahl `RECORD`-Einträge; keine `.pth`-Auswertung oder RECORD-Pfadverfolgung; Quelle unbekannt |
| `cargo-tools` | `CARGO_INSTALL_ROOT`, dann `CARGO_HOME`, sonst `%USERPROFILE%\.cargo`; `.crates2.json`, alternativ `.crates.toml` | Installationsdatensätze; keine Binär-/Nutzungsprüfung, keine Credentials oder Registryquellen; interne Formate experimentell |
| `nuget-cache` | `NUGET_PACKAGES`, sonst `%USERPROFILE%\.nuget\packages`; Versionsordner, `.nupkg.metadata`, `.nuspec` | Keine aktuelle Projektzuordnung allein aus dem Cache; Konfigurations-Overrides außerhalb der Wurzel nicht aufgelöst |

Die Defaults sind Vorschläge, kein Nachweis der wirksamen Managerkonfiguration.
Globale KI-/IDE-Konfiguration, andere Benutzerprofile, WSL und weitere Laufwerke
werden dadurch nicht freigegeben. Die Erfassung startet keine Programme und fragt
keine Registry ab. **Projekte → Funde** zeigt bis zu 200 geschützte Fundstellen
von Umgebungen, Ausgaben und Caches; einzelne Inhalte und Änderungen bleiben ungeprüft.

```powershell
$cli = '.\dist\mogumogu\mogumogu-cli.exe'
& $cli system approve --kind projects --path C:\dev
& $cli scan                         # alle freigegebenen Bereiche
& $cli project candidates
& $cli project accept --scope 1 --relative app   # nur registrieren
& $cli project approve --project 1              # eigener Schritt
& $cli system approve --kind npm-global
& $cli system approve --kind python --path C:\Tools\Python313
& $cli scan
& $cli packages --offset 0 --limit 60
```

IDs durch die tatsächlich zurückgegebenen Werte ersetzen. Bei einem Kandidaten
direkt in der Suchwurzel kann `--relative` entfallen. Ein erneuter vollständiger
Lauf ist nötig, wenn die Suche zuletzt teilweise/fehlgeschlagen war.

Der Paketbericht liefert unter `key` einen stabilen, undurchsichtigen Schlüssel
aus Ökosystem, Name, Version und Quelle. Er kann in PowerShell ohne JSON-Quoting
an `packages keep --key SCHLUESSEL` bzw. `packages unkeep --key SCHLUESSEL`
übergeben werden. Die lesbare Identität steht daneben im Feld `id`.

## Quellenhinweise

Quellenhinweise bleiben standardmäßig aus und werden separat unter Einstellungen
aktiviert. Ein bekanntes öffentliches Paket braucht zusätzlich die Freigabe seines
Hosts. Private Namen werden nie ersatzweise an eine öffentliche Registry gesendet.

| Quelle | Ausgewertete Signale |
|---|---|
| npm | Abkündigungsnachricht der konkreten Version, getrennte `latest`-Abfrage |
| PyPI | Yank der konkreten Release-Dateien; Projektstatus aus Simple JSON: active/archived/deprecated/quarantined; aktuelle Version separat |
| crates.io | Yank der konkreten Version, aktuelle stabile Version; Yank ist keine Aussage über lokale Verwendung |
| NuGet | Abkündigungsgründe, Nachricht und optionale Alternative aus dem Registrierungs-/Katalogdienst; aktuelle stabile Version separat |

Verglichen werden konkrete Versionen nach SemVer (npm/Cargo), PEP 440 (Python)
und normalisierten numerischen NuGet-Versionen einschließlich Prerelease.
Ein neueres Release ist keine Zusage, dass es zu Projekt-Constraints oder Plattform
passt. Änderungen an Manifesten, Migrationen und Tests bleiben eigene Arbeitsschritte.

Antworten sind begrenzt, ohne Credentials/Cookies/Redirects; Quellenhinweise sind
vom Scan getrennt. Cache: ein Tag, nach sieben Tagen veraltet; Fehler verwenden
Backoff und überschreiben frühere Warnungen nicht mit einem Erfolg. Pro Lauf
höchstens 50 Versionsprüfungen und 60 Sekunden Startbudget; eine schon begonnene
Prüfung kann mehrere begrenzte HTTP-Anfragen abschließen. Begrenzte Läufe melden
`partial` und setzen beim nächsten Aufruf mit älteren/ungeprüften Einträgen fort.

## Umfang der Referenzanalyse

- npm: Abhängigkeitstabellen aus Manifest/Lockfile, getrennte Installationspfade,
  lokale Abkündigungsnachrichten, mögliche Befehlsnamen aus npm-Skripten.
- Cargo: Lockfile-Kanten, mehrere Versionen/Quellen, deklarierte Ziel-/Dev-/Build-
  und Workspace-Bezüge, Paket-Aliasse. Keine Feature-/Target-Auswertung.
- uv: grundlegende Lockfile-Kanten; Marker werden nicht ausgewertet.
- NuGet: Framework-Bezug und grundlegende Lockfile-/Nuspec-Kanten; keine MSBuild-
  Auswertung oder vollständige Quellen-/Frameworkauflösung.
- Python: `Requires-Dist` als mögliche Kanten; RECORD nur gezählt, kein Nachweis
  unveränderter oder entbehrlicher Dateien.
- pnpm: Paket-/Store-Schlüssel mit Varianten; vollständige Importer-/Snapshot-
  und Peer-Auflösung bleibt offen.
- GitHub-Workflows: einfache `run:`-Namen/Blöcke als mögliche Befehlsreferenzen.
  Kein YAML-/Shell-Interpreter; Templates, Wrapper, weitere CI-Systeme und
  vollständige Skriptauflösung bleiben offen.
- Vorhandene projektbezogene `npx`-/`uvx`-MCP-Referenzen werden als mögliche
  Paketverweise verknüpft; globale Konfiguration ist weiterhin nicht erfasst.

Auswertung: maximal 200.000 Inventarfakten, 60 Paketidentitäten je Seite und eine
begrenzte Auswahl von Fundstellen/Referenzen je Identität. CLI unterstützt Offset/
Limit, Dashboard Zurück/Weiter. Treffer und Abdeckung sind deshalb ausdrücklich
keine vollständige Liste aller Verbraucher des Rechners.

Wiederholte Reads derselben Seite dürfen einen begrenzten Auswertungscache
verwenden. Jeder Datenbank-Commit, jede Änderung der Uhrzeit oder Seitenauswahl
erzwingt eine neue Bewertung; Behalten-Entscheidungen, Freigaben und veraltete
Quellenhinweise bleiben dadurch aktuell.

Schema v3 erhält vor der Migration eine SQLite-Sicherung. Der alte, nicht
versionsgebundene Quellen-Cache wird invalidiert; Inventar und Entscheidungen
werden nicht in Demo-Datenbanken übernommen. Produktive Bereinigung bleibt G5-gesperrt.
