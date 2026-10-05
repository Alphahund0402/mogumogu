# Unterstützungsmatrix · mogumogu 0.1.0

Stand: 5. Oktober 2026. Statuswerte nach PROJEKTPLAN §8.5: `geplant`, `experimentell`, `getestet`, `eingeschränkt`, `nicht unterstützt`.
„Experimentell“ heißt: Parser und Fixtures existieren und laufen in der Testsuite; eine Abnahme gegen reale Toolversionen auf Windows 10 **und** 11 steht aus. „Unterstützt“ gilt nur innerhalb der genannten Formate.

Getestete Plattform dieses Stands: Windows 10 Pro 22H2 (Build 19045), x64, Rust 1.98.0, Slint 1.18.1 (winit, Software-Renderer), SQLite 3.53.2 (gebündelt über rusqlite 0.40.2). Windows 11 ist **nicht** abgenommen.

Linux/Docker unterstützt die portablen Entwicklungsprüfungen, keine Inventar-Laufzeit.
Tray, Named Pipes, Job Objects und Windows-Lese-/Löschgrenzen bleiben dort nicht
unterstützt. Anleitung: [DOCKER.md](DOCKER.md).

## Desktop und Datenmodus

Der normale Start (EXE, `build.ps1`, `start.ps1`, `build.cmd`) verwendet das lokale Inventar. Es zeigt ausschließlich registrierte Metadaten und statische Erfassungen aus freigegebenen Bereichen; ein neues Inventar ist leer. Beispieldaten erfordern ausdrücklich `--demo` bzw. `-Mode Demo` und liegen in einer getrennten Datenbank.

Navigation, Filter, Schaltflächen und Listenzeilen verwenden gemeinsame Auswahl-, Hover-, Druck- und Fokuszustände. Häkchen, Seitenmarkierung und Schriftgewicht ergänzen die Auswahlfarbe; Tastaturfokus liegt innerhalb des Elements. Dies ersetzt keine vollständige Prüfung mit Assistenztechnik.

## Paketmanager

Alle Adapter lesen ausschließlich statische Dateien in freigegebenen Bereichen. Kein Adapter startet den Manager, einen Interpreter, Skripte oder Lifecycle-Hooks, und keiner besitzt eine Bereinigungsfähigkeit.

| Manager | Inventar | Updatehinweise | Bereinigung | Formate (Fixtures) | Grenzen |
|---|---|---|---|---|---|
| npm | experimentell | experimentell | nicht unterstützt | package.json (deklariert), package-lock.json v1–v3 (aufgelöst, mit Quelle), node_modules/.package-lock.json (installiert) | Benutzer-.npmrc und globale Installationen werden nicht gelesen |
| pnpm | experimentell | nicht unterstützt | nicht unterstützt | pnpm-lock.yaml v5–v9 (Paketschlüssel), pnpm-workspace.yaml, node_modules/.pnpm (installiert) | Lockfile nennt keine Registry → Quelle unbekannt → keine Abfrage |
| pip | experimentell | eingeschränkt | nicht unterstützt | requirements*.txt, pyproject.toml (PEP 621, dependency-groups, Poetry), venv über pyvenv.cfg + *.dist-info | Kein Interpreterstart (.pth); pip.conf außerhalb nicht gelesen → ohne Indexangabe Quelle unbekannt |
| uv | experimentell | experimentell | nicht unterstützt | uv.lock (Quellen explizit), uv.toml, pyproject.toml mit uv.lock; venv mit `uv`-Kennung | uv-Cache wird nie verändert |
| Cargo | experimentell | experimentell | nicht unterstützt | Cargo.toml (inkl. Ziel-/Workspace-Tabellen), Cargo.lock v3/v4, target/ als Ausgabe | Kein `cargo metadata` (würde Buildskripte ausführen) |
| NuGet | experimentell | nicht unterstützt | nicht unterstützt | SDK-Projektdateien (bedingte Referenzen), Directory.Packages.props, packages.config, packages.lock.json, obj/project.assets.json | NuGet.config-Quellen nicht ausgewertet → keine Abfrage |
| WinGet | eingeschränkt | nicht unterstützt | nicht unterstützt | Windows-Softwareliste (Uninstall-Schlüssel HKCU/HKLM 64/32 Bit) | `winget list` wird nicht gestartet (Netzwerk); Verwaltung durch WinGet wird nicht aus Namen abgeleitet |
| Scoop | experimentell | nicht unterstützt | nicht unterstützt | apps/&lt;app&gt;/&lt;version&gt;/install.json + manifest.json, persist/, cache/ | `current`-Junction wird nie verfolgt |
| Chocolatey | experimentell | nicht unterstützt | nicht unterstützt | lib/&lt;paket&gt;/&lt;paket&gt;.nuspec, lib-bad/ | Kein Paket-PowerShell |

Updatehinweise sind standardmäßig aus. Abgefragt wird nur, wenn (1) der globale Schalter an ist, (2) die Quelle eines aufgelösten oder installierten Pakets als öffentliche Registry belegt ist (registry.npmjs.org, pypi.org, crates.io) und (3) genau dieser Host lokal freigegeben ist. Private, Pfad-, Git- und unbekannte Quellen werden nie an öffentliche Dienste gemeldet. Fehler ergeben `offline`/`unbekannt`, nie „aktuell“; Ergebnisse älter als sieben Tage gelten als `veraltet`.

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

Benutzer- und organisationsweite Einstellungen außerhalb freigegebener Projektordner werden nicht gelesen; die Wirksamkeitsanzeige ist daher stets „statisch ermittelt, unvollständig“. Eine `npx`-/`uvx`-Referenz ist kein Installations- oder Nutzungsnachweis.

**Erweiterung ohne Kernumbau:** Ein neues Format mit vorhandenem Parser wird als Eintrag im Katalog ergänzt. Der Katalog wird beim Build eingebettet und beim Laden validiert (relative Muster, kein `..`/`**`, keine unbekannten Felder wie Kommandos, Limits nur verschärfend). Test: `contributed_profile_without_core_change_and_rejections`.

## Bereinigung

| Ressourcentyp | Stand |
|---|---|
| Verwaltete Temp-Ausgabe (`temporary/` eines von mogumogu angelegten Scratchpads) in einer registrierten Wegwerf-Testwurzel | Testexecutor freigegeben (G-TEST-CLEANUP); Ordner selbst bleibt |
| Dieselbe Ausgabe außerhalb einer Testwurzel | gesperrt bis zum unabhängigen Review (G5) |
| Quellen, Ergebnisse, Umgebungen, Caches, Paketspeicher, KI-Konfiguration, unbekannte Inhalte | nicht unterstützt (immer geschützt) |
