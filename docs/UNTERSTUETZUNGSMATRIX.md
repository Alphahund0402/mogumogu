# Unterstützungsmatrix · mogumogu 0.1.0

Stand: 18. Oktober 2026. Statuswerte nach PROJEKTPLAN §8.5: `geplant`, `experimentell`, `getestet`, `eingeschränkt`, `nicht unterstützt`.
„Experimentell“ heißt: Parser und Fixtures existieren und laufen in der Testsuite; eine Abnahme gegen reale Toolversionen auf Windows 10 **und** 11 steht aus. „Unterstützt“ gilt nur innerhalb der genannten Formate.

Getestete Plattform dieses Stands: Windows 10 Pro 22H2 (Build 19045), x64, Rust 1.98.0, Slint 1.18.1 (winit, Software-Renderer), SQLite 3.53.2 (gebündelt über rusqlite 0.40.2). Windows 11 ist **nicht** abgenommen.

Linux/Docker unterstützt die portablen Entwicklungsprüfungen und baut die Windows-Programme mit der GNU-Toolchain (experimentell; maßgeblich bleibt der native MSVC-Build), keine Inventar-Laufzeit.
Tray, Named Pipes, Job Objects und Windows-Lese-/Löschgrenzen bleiben dort nicht
unterstützt. Anleitung: [DOCKER.md](DOCKER.md).

## Desktop und Datenmodus

Der normale Start (EXE, `build.ps1`, `start.ps1`, `build.cmd`) verwendet das lokale Inventar. Es zeigt ausschließlich registrierte Metadaten und statische Erfassungen aus freigegebenen Bereichen; ein neues Inventar ist leer. Beispieldaten erfordern ausdrücklich `--demo` bzw. `-Mode Demo` und liegen in einer getrennten Datenbank.

Navigation, Filter, Schaltflächen und Listenzeilen verwenden gemeinsame Auswahl-, Hover-, Druck- und Fokuszustände. Häkchen, Seitenmarkierung und Schriftgewicht ergänzen die Auswahlfarbe; Tastaturfokus liegt innerhalb des Elements. Dies ersetzt keine vollständige Prüfung mit Assistenztechnik.

## Automatische Systemerkennung

Der **Systembefund** (`system findings`, Ansicht **System**) ist experimentell: 23 bekannte Befehle im Suchpfad mit Nachweisart (Pfad belegt, Shim, App-Alias, Verknüpfung), bekannte Orte globaler Bestände und Benutzerordner von KI-Clients. Nur Metadaten (`symlink_metadata`), nur lokale feste Laufwerke, kein Programmstart, kein Folgen von Verknüpfungen. Ein Fund belegt weder Funktionsfähigkeit noch Version noch den verwaltenden Manager.

Separat freizugebende Bereiche: `projects` (Dateinamensuche), `npm-global`, `uv-tools`, `python`, `cargo-tools`, `nuget-cache` und `user-ai` (persönliche KI-Konfiguration bekannter Clients). Kandidaten werden weder automatisch registriert noch zum Inhaltslesen freigegeben. Globale Bestände werden anhand enger Metadaten-Selektoren erfasst. Details und Grenzen: [PAKETPRUEFUNG](PAKETPRUEFUNG.md), [SYSTEMERKENNUNG](SYSTEMERKENNUNG.md). Noch offen: uv-Python, rustup, .NET-Tools und pyenv als erfasste Bestände (bisher nur Ortsnachweis), Konfigurations-Overrides, Git-Konfiguration und die automatische Bestimmung eines Scratchpad-Zwecks. Der Adapterkatalog belegt keine Managerinstallation.

## Paketmanager

Alle Adapter lesen ausschließlich statische Dateien in freigegebenen Bereichen. Kein Adapter startet den Manager, einen Interpreter, Skripte oder Lifecycle-Hooks, und keiner besitzt eine Bereinigungsfähigkeit.

| Manager | Inventar | Bereinigung | Formate (Fixtures) | Grenzen |
|---|---|---|---|---|
| npm | experimentell | nicht unterstützt | package.json, package-lock v1–v3, verstecktes Installations-Lockfile; globales package.json mit eigener Freigabe | Benutzer-.npmrc nicht gelesen; Quelle global ohne `_resolved` unbekannt |
| pnpm | experimentell | nicht unterstützt | pnpm-lock.yaml v5–v9 (Paketschlüssel), pnpm-workspace.yaml, node_modules/.pnpm (installiert) | Lockfile nennt keine Registry → Quelle unbekannt → keine Abfrage |
| pip | experimentell | nicht unterstützt | requirements*.txt, pyproject.toml (PEP 621, dependency-groups, Poetry), venv über pyvenv.cfg + *.dist-info | Kein Interpreterstart (.pth); pip.conf außerhalb nicht gelesen → ohne Indexangabe Quelle unbekannt |
| uv | experimentell | nicht unterstützt | uv.lock (Quellen explizit), uv.toml, pyproject.toml mit uv.lock; venv mit `uv`-Kennung | uv-Cache wird nie verändert |
| Cargo | experimentell | nicht unterstützt | Cargo.toml, Cargo.lock v3/v4, target/; `.crates.toml`/`.crates2.json` mit eigener Freigabe | Kein Cargo-Aufruf; globale Installationsdatensätze belegen keine aktuelle CLI-Nutzung; interne Formate nicht stabil |
| NuGet | experimentell | nicht unterstützt | Projekt-/Lock-/Assets-Dateien; Cache mit `.nupkg.metadata`/`.nuspec` nach eigener Freigabe | Projektquellen unbekannt |
| Bun | experimentell | nicht unterstützt | bun.lock v1 (Text, JSONC), package.json daneben, node_modules (Ordnernamen) | bun.lockb (binär) und bunfig.toml nicht gelesen; leere Registry-Angabe → Quelle unbekannt |
| Yarn | experimentell | nicht unterstützt | yarn.lock v1 und Berry (v2+) zeilenweise, package.json daneben, node_modules (Ordnernamen) | .yarnrc.yml nicht gelesen → Berry-Quellen unbekannt; Plug'n'Play-Cache nicht inventarisiert |
| Go-Module | experimentell | nicht unterstützt | go.mod (require direkt/indirekt, replace), go.sum (Prüfsummen des Modulinhalts), vendor/modules.txt (installiert) | Kein go-Aufruf; GOPROXY/GOPRIVATE nicht gelesen → Quelle unbekannt außer lokalem replace; go.sum kann alte Versionen enthalten; go.work nicht ausgewertet |
| Gradle | experimentell | nicht unterstützt | libs.versions.toml, gradle.lockfile, Koordinaten als Text in build.gradle(.kts); build/ als Ausgabe | Buildskripte werden nicht ausgewertet → Versionen aus Variablen, BOMs, Plugins und `libs.`-Verweisen bleiben offen |
| Maven | experimentell | nicht unterstützt | pom.xml (dependencies, dependencyManagement, plugins, parent); target/ als Ausgabe | Platzhalter nur aus Properties derselben Datei; Elternprojekte, Profile und settings.xml nicht ausgewertet |

Eine package.json zählt zu dem Manager, dessen Lockfile daneben liegt (pnpm, Bun, Yarn, sonst npm); Workspace-Mitglieder ohne eigenes Lockfile zählen deshalb zu npm. Bun- und Yarn-Pakete teilen die npm-Paketidentität, Gradle und Maven die Maven-Identität; Belege und Behalten-Entscheidungen gelten damit managerübergreifend. Go-Modulcache und Bun-Cache sind noch keine messbaren Paket-Caches (die Cache-Spalte von Schema v6 lässt neue Arten nur mit einer Migration zu).

Registries werden nie abgefragt; mogumogu stellt keine Netzwerkverbindungen her. Abkündigungen erscheinen nur, wenn ein lokales Lockfile sie vermerkt. WinGet-, Scoop- und Chocolatey-Listen werden nicht mehr erfasst; bestehende Freigaben dafür widerruft mogumogu beim Start einmal.

## KI-Profile

Erkennung über den deklarativen Katalog [`profiles/ai-catalog.toml`](../profiles/ai-catalog.toml) (Version 2026-10-20.1). Alle Profile sind `experimentell`: Die Pfade sind Erkennungskandidaten aus der jeweiligen Herstellerdokumentation; „Datei vorhanden“, „Client installiert“, „Format verstanden“ und „tatsächlich verwendet“ bleiben getrennte Aussagen. Nichts wird ausgeführt, geladen oder befolgt.

| Profil | Erkannte Dateien | Ausgewertet |
|---|---|---|
| Agent Skills | `.agents/skills/*/SKILL.md` | Name, Beschreibung (gekürzt, redigiert), relative Verweise mit Existenzprüfung |
| Codex | `AGENTS.md`, `AGENTS.override.md`, `.codex/config.toml` | Überschrift, Zeilen, Verweise; MCP-Server (Name, Programm, Paketreferenz); `sandbox_mode`, `approval_policy`, Zahl der `writable_roots` |
| Claude Code | `CLAUDE.md`, `CLAUDE.local.md`, `.claude/CLAUDE.md`, `.claude/skills/*/SKILL.md`, `.claude/{agents,commands,rules}/*.md`, `.claude/settings(.local).json`, `.mcp.json` | Hooks/Berechtigungen nur gezählt, MCP als Referenz; `defaultMode`, Zahl der `additionalDirectories` (davon außerhalb), Read-/Edit-Verbote, `sandbox.enabled` |
| GitHub Copilot / VS Code | `.github/copilot-instructions.md`, `.github/instructions/*.instructions.md`, `.github/prompts/*.prompt.md`, `.github/agents/*.md`, `.github/skills/*/SKILL.md`, `.vscode/mcp.json`, `AGENTS.md` | wie oben, JSONC |
| Cursor | `.cursor/rules/*.mdc`, `.cursor/rules/*.md`, `.cursorrules`, `.cursor/mcp.json`, `.cursorignore`, `.cursorindexingignore`, `AGENTS.md` | deklariertes `alwaysApply`, Dateimuster vorhanden; Ignore-Regeln gezählt |
| Gemini CLI | `GEMINI.md`, `.gemini/settings.json`, `.gemini/skills/*/SKILL.md`, `.geminiignore` | `@`-Imports als Verweise; `sandbox`; Ignore-Regeln gezählt |
| Windsurf / Cascade | `.windsurf/rules/*.md`, `.windsurfrules`, `.codeiumignore` | Vorhandensein, Zeilen; Ignore-Regeln gezählt |
| Cline | `.clinerules` (Datei), `.clinerules/*.md` | Vorhandensein, Zeilen |
| Roo Code | `.roo/rules/*.md`, `.roo/rules-*/*.md`, `.roomodes`, `.roo/mcp.json` | Modusregeln getrennt |
| OpenCode | `opencode.json(c)`, `.opencode/{agent,command}/*.md`, `AGENTS.md` | MCP-Einträge (`mcp`) |
| Continue | `.continue/rules/*.md`, `.continue/config.yaml`, `.continue/mcpServers/*.yaml` | Vorhandensein |
| Aider | `.aider.conf.yml`, `.aiderignore` | Vorhandensein; Ignore-Regeln gezählt |

**Kontextfilter:** experimentell; schreibt nach Vorschau und Bestätigung nur `permissions.deny` in die persönliche `.claude/settings.local.json` eines Projekts mit Lesefreigabe (Leseregeln für gefundene Umgebungen, Ausgaben, Caches, Testdaten und Schlüsseldateien), für ein Projekt oder alle zusammen. Verbote aus `.claude/settings.json` werden gelesen und nicht wiederholt. Geschrieben wird über den gehaltenen `.claude`-Handle; ein Ziel, das eine Verknüpfung ist oder weitere Hardlinks hat, wird abgelehnt. Keine anderen Dateien, keine geteilten oder Git-Dateien, keine Benutzereinstellungen, keine Codex-Konfiguration. Claude Code wendet die Regeln auf seine Datei-Werkzeuge, erkannte Shell-Dateibefehle (`cat`, `head`, `tail`, `sed`, `tee`) und Umleitungen an, nicht auf Programme, die Dateien selbst öffnen; eine Sandbox, die das systemweit erzwingt, gibt es unter nativem Windows nicht.

**Persönliche Konfiguration (`user-ai`):** Für die Benutzerordner `.claude`, `.codex`, `.agents`, `.gemini`, `.cursor`, `.continue`, `.roo`, `.config\opencode` und `%APPDATA%\Code\User` gelten dieselben Katalogmuster, angewandt auf den Benutzerordner (z. B. `%USERPROFILE%\.claude\skills\*\SKILL.md`). Die Erfassung betritt nur Unterordner, die ein Muster erreichen kann; Verläufe, Sitzungen, Caches und Zugangsdateien bleiben ungelesen. Die Anzeige trennt Projekt und Benutzer. Organisationsweite Einstellungen werden nicht gelesen, und der Vorrang zwischen Benutzer- und Projektebene wird nicht aufgelöst; die Wirksamkeitsanzeige bleibt „statisch ermittelt, unvollständig“. Eine `npx`-/`uvx`-Referenz ist kein Installations- oder Nutzungsnachweis.

**Erweiterung ohne Kernumbau:** Ein neues Format mit vorhandenem Parser wird als Eintrag im Katalog ergänzt. Der Katalog wird beim Build eingebettet und beim Laden validiert (relative Muster, kein `..`/`**`, keine unbekannten Felder wie Kommandos, Limits nur verschärfend). Test: `contributed_profile_without_core_change_and_rejections`.

## Bestehende Scratchpads

`scratch register`, `scratch adopt` und `scratch adopt-found` sind implementiert; `scratch create` und verwaltete Läufe wurden am 20.10.2026 entfernt. Die experimentelle Fundliste erkennt in freigegebenen Such- und Projektbereichen sowie in ganzen lokalen Festplatten (Laufwerkssuche) Projekte mit Sicherheit und Teilprojekten, mögliche Scratchpads nach Versuchswörtern, Vorlagen- und Kopiennamen oder losen Skripten, bekannte Ablageorte von Werkzeugen nach eigener Freigabe, lose Skripte, Notebooks, Umgebungen, Abhängigkeiten, Build-/Testausgaben, Testdaten, Datenbanken, bekannte Cache-Namen und Ordner mit `CACHEDIR.TAG` (deren Inhalt wird nicht betreten). Teilprojekte stehen nach den äußeren Projekten derselben Sicherheit. Pfade werden je Ordnername redigiert, damit ein gewöhnlicher langer Pfad registrierbar bleibt; in einem Namen gilt nur ein langes Wort aus Buchstaben und Ziffern als Token, Commit-IDs, Prüfsummen und GUIDs (Hex in einer Schreibweise) nicht, bekannte Tokenformate weiterhin schon. Projektbezüge aus Pfaden bleiben vermutet; ausdrückliche Zuordnung mit Zweck ist auch für einzelne Dateien möglich. Namen und Änderungsdatum belegen keine Entbehrlichkeit. Die Liste ist auf 200 Funde begrenzt; die globale Suche erreicht weitere gespeicherte Funde. Editor-Backups, vollständige Worktree-Auflösung und automatische Zweckbestimmung bleiben offen. `scratch_root` ist der Zielort für neue Scratchpads, keine Suchfreigabe. Details: [Entwicklungsfunde](ENTWICKLUNGSFUNDE.md).

## Speicher und Inhaltsansicht

| Fähigkeit | Stand und Grenze |
|---|---|
| Paket-Caches | experimentell; npm, pip, uv, Cargo-Registry, Gradle, Maven, pnpm, Yarn mit eigener Freigabe (Schema v6), nur gemessen; im Aufräumbericht mit Aufräumbefehl des Werkzeugs, Maven als „Prüfen“; kein Paketbestand, nichts wird ausgeführt oder entfernt |
| Aufräumbericht | experimentell; nur in Projekten mit Lesefreigabe und freigegebenen globalen Beständen; ein begrenzter Durchgang je Projekt (2 Mio. Einträge, 120 s), nur Verzeichniseinträge; „wiederherstellbar“ nur mit Wiederherstellungsbeleg und, wo der Name allein nicht reicht, der Spur des Werkzeugs im Ordner; Bericht im Speicher des Besitzers, nicht in der Datenbank; entfernt nichts |
| Laufwerksdashboard | experimentell; Windows-Laufwerksmetadaten und gespeicherte logische Projektgrößen, unbekannte/offline Kapazitäten bleiben unbekannt; verschachtelte/identische Pfade nicht doppelt summiert; keine physische Hardlink-/Kompressionsbilanz |
| Skill-/Instruktionsinhalte | experimentell; frische kataloggebundene Leseprüfung, schreibgeschützter Text mit Redaktion bekannter sensibler Muster; maximal 128 KiB bzw. Profillimit; keine Inhaltsdaten in DB, Logs, Snapshot oder Export; gemischte Konfiguration nicht anzeigbar |

Gezielte Windows-Fixtures: `tests/package_scopes.rs`, `tests/disk.rs`, `tests/ai_content.rs`. Getrennter Agentenreview der neuen Lesewege erfolgt; dies ersetzt keine menschliche Sicherheitsabnahme oder Windows-11-Abnahme.

## Bereinigung

| Ressourcentyp | Stand |
|---|---|
| Wiederherstellbar belegter Ordner eines freigegebenen Projekts (`node_modules`/`.venv` mit Lockfile, `target`/`dist`/`build` mit Build-Datei und Werkzeugspur, Werkzeugcaches, Test-Ausgaben) in einer registrierten Wegwerf-Testwurzel, ohne `.git`, `.mogumogu-keep`, Verknüpfungen oder Platzhalter darin | Papierkorb-Plan ausführbar (G-TEST-CLEANUP); Ordner als Ganzes in den Papierkorb |
| Ordner mit Verknüpfungen darin (pnpm, Workspaces), mit mehrdeutigem Namen (Punkt oder Leerzeichen am Ende) oder Pfad über 259 Zeichen | nicht planbar |
| Derselbe Ordner außerhalb einer Testwurzel | Plan sichtbar, Bestätigung gesperrt bis G5 |
| Alles unter „Prüfen“ (Testdaten, Datenbanken, Logs, große Dateien, Umgebungen und Ausgaben ohne Datei daneben), Quellen, Lockfiles, `.git`, globale Paketspeicher, KI-Konfiguration, unbekannte Inhalte | nicht planbar (immer geschützt) |
