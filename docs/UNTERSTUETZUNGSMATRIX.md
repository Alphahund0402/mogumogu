# Unterstützungsmatrix · mogumogu 0.1.0

Stand: 18. Oktober 2026. Statuswerte nach PROJEKTPLAN §8.5: `geplant`, `experimentell`, `getestet`, `eingeschränkt`, `nicht unterstützt`.
„Experimentell“ heißt: Parser und Fixtures existieren und laufen in der Testsuite; eine Abnahme gegen reale Toolversionen auf Windows 10 **und** 11 steht aus. „Unterstützt“ gilt nur innerhalb der genannten Formate.

Getestete Plattform dieses Stands: Windows 10 Pro 22H2 (Build 19045), x64, Rust 1.98.0, Slint 1.18.1 (winit, Software-Renderer), SQLite 3.53.2 (gebündelt über rusqlite 0.40.2). Windows 11 ist **nicht** abgenommen.

Linux/Docker unterstützt die portablen Entwicklungsprüfungen, keine Inventar-Laufzeit.
Tray, Named Pipes, Job Objects und Windows-Lese-/Löschgrenzen bleiben dort nicht
unterstützt. Anleitung: [DOCKER.md](DOCKER.md).

## Desktop und Datenmodus

Der normale Start (EXE, `build.ps1`, `start.ps1`, `build.cmd`) verwendet das lokale Inventar. Es zeigt ausschließlich registrierte Metadaten und statische Erfassungen aus freigegebenen Bereichen; ein neues Inventar ist leer. Beispieldaten erfordern ausdrücklich `--demo` bzw. `-Mode Demo` und liegen in einer getrennten Datenbank.

Navigation, Filter, Schaltflächen und Listenzeilen verwenden gemeinsame Auswahl-, Hover-, Druck- und Fokuszustände. Häkchen, Seitenmarkierung und Schriftgewicht ergänzen die Auswahlfarbe; Tastaturfokus liegt innerhalb des Elements. Dies ersetzt keine vollständige Prüfung mit Assistenztechnik.

## Automatische Systemerkennung

Der **Systembefund** (`system findings`, Ansicht **System**) ist experimentell: 21 bekannte Befehle im Suchpfad mit Nachweisart (Pfad belegt, Shim, App-Alias, Verknüpfung), bekannte Orte globaler Bestände und Benutzerordner von KI-Clients. Nur Metadaten (`symlink_metadata`), nur lokale feste Laufwerke, kein Programmstart, kein Folgen von Verknüpfungen. Ein Fund belegt weder Funktionsfähigkeit noch Version noch den verwaltenden Manager.

Separat freizugebende Bereiche: `projects` (Dateinamensuche), `npm-global`, `uv-tools`, `python`, `cargo-tools`, `nuget-cache` und `user-ai` (persönliche KI-Konfiguration bekannter Clients). Kandidaten werden weder automatisch registriert noch zum Inhaltslesen freigegeben. Globale Bestände werden anhand enger Metadaten-Selektoren erfasst. Details und Grenzen: [PAKETPRUEFUNG](PAKETPRUEFUNG.md), [SYSTEMERKENNUNG](SYSTEMERKENNUNG.md). Noch offen: uv-Python, rustup, .NET-Tools und pyenv als erfasste Bestände (bisher nur Ortsnachweis), Konfigurations-Overrides, Git-Konfiguration und die automatische Bestimmung eines Scratchpad-Zwecks. Der Adapterkatalog belegt keine Managerinstallation.

## Paketmanager

Alle Adapter lesen ausschließlich statische Dateien in freigegebenen Bereichen. Kein Adapter startet den Manager, einen Interpreter, Skripte oder Lifecycle-Hooks, und keiner besitzt eine Bereinigungsfähigkeit.

| Manager | Inventar | Updatehinweise | Bereinigung | Formate (Fixtures) | Grenzen |
|---|---|---|---|---|---|
| npm | experimentell | experimentell | nicht unterstützt | package.json, package-lock v1–v3, verstecktes Installations-Lockfile; globales package.json mit eigener Freigabe | Benutzer-.npmrc nicht gelesen; Quelle global ohne `_resolved` unbekannt |
| pnpm | experimentell | nicht unterstützt | nicht unterstützt | pnpm-lock.yaml v5–v9 (Paketschlüssel), pnpm-workspace.yaml, node_modules/.pnpm (installiert) | Lockfile nennt keine Registry → Quelle unbekannt → keine Abfrage |
| pip | experimentell | eingeschränkt | nicht unterstützt | requirements*.txt, pyproject.toml (PEP 621, dependency-groups, Poetry), venv über pyvenv.cfg + *.dist-info | Kein Interpreterstart (.pth); pip.conf außerhalb nicht gelesen → ohne Indexangabe Quelle unbekannt |
| uv | experimentell | experimentell | nicht unterstützt | uv.lock (Quellen explizit), uv.toml, pyproject.toml mit uv.lock; venv mit `uv`-Kennung | uv-Cache wird nie verändert |
| Cargo | experimentell | experimentell | nicht unterstützt | Cargo.toml, Cargo.lock v3/v4, target/; `.crates.toml`/`.crates2.json` mit eigener Freigabe | Kein Cargo-Aufruf; globale Installationsdatensätze belegen keine aktuelle CLI-Nutzung; interne Formate nicht stabil |
| NuGet | experimentell | eingeschränkt | nicht unterstützt | Projekt-/Lock-/Assets-Dateien; Cache mit `.nupkg.metadata`/`.nuspec` nach eigener Freigabe | Projektquellen unbekannt; Quellenabfrage nur bei belegter `api.nuget.org`-Quelle im Cache |
| WinGet | eingeschränkt | nicht unterstützt | nicht unterstützt | Windows-Softwareliste (Uninstall-Schlüssel HKCU/HKLM 64/32 Bit) | `winget list` wird nicht gestartet (Netzwerk); Verwaltung durch WinGet wird nicht aus Namen abgeleitet |
| Scoop | experimentell | nicht unterstützt | nicht unterstützt | apps/&lt;app&gt;/&lt;version&gt;/install.json + manifest.json, persist/, cache/ | `current`-Junction wird nie verfolgt |
| Chocolatey | experimentell | nicht unterstützt | nicht unterstützt | lib/&lt;paket&gt;/&lt;paket&gt;.nuspec, lib-bad/ | Kein Paket-PowerShell |

Quellenhinweise sind standardmäßig aus. Nur belegte öffentliche Quellen werden nach globaler und hostbezogener Freigabe abgefragt: `registry.npmjs.org`, `pypi.org`, `crates.io`, `api.nuget.org`. Private, Pfad-, Git- und unbekannte Quellen werden nie an öffentliche Dienste gemeldet. Version und Quelle gehören zum Cache-Schlüssel; gleiche Namen verschiedener Projekte überschreiben keine Version. Neuere Version, Abkündigung, Archivierung und Yank sind getrennte Fakten. Fehler ergeben `offline`/`unbekannt`; ältere Warnungen bleiben mit Datum und Unsicherheit sichtbar. Nach sieben Tagen gilt eine erfolgreiche Quellenabfrage als veraltet. Kein Paketmanager wird für diese Abfragen gestartet.

## KI-Profile

Erkennung über den deklarativen Katalog [`profiles/ai-catalog.toml`](../profiles/ai-catalog.toml) (Version 2026-10-04.1). Alle Profile sind `experimentell`: Die Pfade sind Erkennungskandidaten aus der jeweiligen Herstellerdokumentation; „Datei vorhanden“, „Client installiert“, „Format verstanden“ und „tatsächlich verwendet“ bleiben getrennte Aussagen. Nichts wird ausgeführt, geladen oder befolgt.

| Profil | Erkannte Dateien | Ausgewertet |
|---|---|---|
| Agent Skills | `.agents/skills/*/SKILL.md` | Name, Beschreibung (gekürzt, redigiert), relative Verweise mit Existenzprüfung |
| Codex | `AGENTS.md`, `AGENTS.override.md`, `.codex/config.toml` | Überschrift, Zeilen, Verweise; MCP-Server (Name, Programm, Paketreferenz) |
| Claude Code | `CLAUDE.md`, `CLAUDE.local.md`, `.claude/CLAUDE.md`, `.claude/skills/*/SKILL.md`, `.claude/{agents,commands,rules}/*.md`, `.claude/settings(.local).json`, `.mcp.json` | Hooks/Berechtigungen nur gezählt, MCP als Referenz |
| GitHub Copilot / VS Code | `.github/copilot-instructions.md`, `.github/instructions/*.instructions.md`, `.github/prompts/*.prompt.md`, `.github/agents/*.md`, `.github/skills/*/SKILL.md`, `.vscode/mcp.json`, `AGENTS.md` | wie oben, JSONC |
| Cursor | `.cursor/rules/*.mdc`, `.cursor/rules/*.md`, `.cursorrules`, `.cursor/mcp.json`, `AGENTS.md` | deklariertes `alwaysApply`, Dateimuster vorhanden |
| Gemini CLI | `GEMINI.md`, `.gemini/settings.json`, `.gemini/skills/*/SKILL.md` | `@`-Imports als Verweise |
| Windsurf / Cascade | `.windsurf/rules/*.md`, `.windsurfrules` | Vorhandensein, Zeilen |
| Cline | `.clinerules` (Datei), `.clinerules/*.md` | Vorhandensein, Zeilen |
| Roo Code | `.roo/rules/*.md`, `.roo/rules-*/*.md`, `.roomodes`, `.roo/mcp.json` | Modusregeln getrennt |
| OpenCode | `opencode.json(c)`, `.opencode/{agent,command}/*.md`, `AGENTS.md` | MCP-Einträge (`mcp`) |
| Continue | `.continue/rules/*.md`, `.continue/config.yaml`, `.continue/mcpServers/*.yaml` | Vorhandensein |
| Aider | `.aider.conf.yml` | Vorhandensein |

**Persönliche Konfiguration (`user-ai`):** Für die Benutzerordner `.claude`, `.codex`, `.agents`, `.gemini`, `.cursor`, `.continue`, `.roo`, `.config\opencode` und `%APPDATA%\Code\User` gelten dieselben Katalogmuster, angewandt auf den Benutzerordner (z. B. `%USERPROFILE%\.claude\skills\*\SKILL.md`). Die Erfassung betritt nur Unterordner, die ein Muster erreichen kann; Verläufe, Sitzungen, Caches und Zugangsdateien bleiben ungelesen. Die Anzeige trennt Projekt und Benutzer. Organisationsweite Einstellungen werden nicht gelesen, und der Vorrang zwischen Benutzer- und Projektebene wird nicht aufgelöst; die Wirksamkeitsanzeige bleibt „statisch ermittelt, unvollständig“. Eine `npx`-/`uvx`-Referenz ist kein Installations- oder Nutzungsnachweis.

**Erweiterung ohne Kernumbau:** Ein neues Format mit vorhandenem Parser wird als Eintrag im Katalog ergänzt. Der Katalog wird beim Build eingebettet und beim Laden validiert (relative Muster, kein `..`/`**`, keine unbekannten Felder wie Kommandos, Limits nur verschärfend). Test: `contributed_profile_without_core_change_and_rejections`.

## Bestehende Scratchpads

`scratch create`, `scratch register` und `scratch adopt` sind implementiert. Die experimentelle Fundliste erkennt in freigegebenen Such- und Projektbereichen Projekte, kontextbezogene Experimentordner, lose Skripte, Notebooks, Umgebungen, Abhängigkeiten, Build-/Testausgaben, Testdaten, Datenbanken und bekannte Cache-Namen. Projektbezüge aus Pfaden bleiben vermutet; ausdrückliche Zuordnung mit Zweck ist auch für einzelne Dateien möglich. Namen und Änderungsdatum belegen keine Entbehrlichkeit. Die Liste ist auf 200 Funde begrenzt; die globale Suche erreicht weitere gespeicherte Funde. Editor-Backups, vollständige Worktree-Auflösung und automatische Zweckbestimmung bleiben offen. `scratch_root` ist der Zielort für neue Scratchpads, keine Suchfreigabe. Details: [Entwicklungsfunde](ENTWICKLUNGSFUNDE.md).

## Speicher und Inhaltsansicht

| Fähigkeit | Stand und Grenze |
|---|---|
| Laufwerksdashboard | experimentell; Windows-Laufwerksmetadaten und gespeicherte logische Projektgrößen, unbekannte/offline Kapazitäten bleiben unbekannt; verschachtelte/identische Pfade nicht doppelt summiert; keine physische Hardlink-/Kompressionsbilanz |
| Ordnerstruktur | experimentell; auf Anfrage je eine Ebene, navigierbar innerhalb identitätsgebundener Projekt-/Suchfreigaben; maximal 200 Einträge; Messbudget 10.000 Einträge/2 s; reine Suchfreigaben zeigen keine Größen; keine Links/Platzhalter, kein allgemeiner Browser unter `user-ai` |
| Skill-/Instruktionsinhalte | experimentell; frische kataloggebundene Leseprüfung, schreibgeschützter Text mit Redaktion bekannter sensibler Muster; maximal 128 KiB bzw. Profillimit; keine Inhaltsdaten in DB, Logs, Snapshot oder Export; gemischte Konfiguration nicht anzeigbar |

Gezielte Windows-Fixtures: `tests/package_scopes.rs`, `tests/disk.rs`, `tests/ai_content.rs`. Getrennter Agentenreview der neuen Lesewege erfolgt; dies ersetzt keine menschliche Sicherheitsabnahme oder Windows-11-Abnahme.

## Bereinigung

| Ressourcentyp | Stand |
|---|---|
| Verwaltete Temp-Ausgabe (`temporary/` eines von mogumogu angelegten Scratchpads) in einer registrierten Wegwerf-Testwurzel | Testexecutor freigegeben (G-TEST-CLEANUP); Ordner selbst bleibt |
| Dieselbe Ausgabe außerhalb einer Testwurzel | gesperrt bis zum unabhängigen Review (G5) |
| Quellen, Ergebnisse, Umgebungen, Caches, Paketspeicher, KI-Konfiguration, unbekannte Inhalte | nicht unterstützt (immer geschützt) |
