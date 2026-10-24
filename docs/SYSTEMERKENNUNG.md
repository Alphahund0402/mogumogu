# Systemerkennung für mogumogu

Recherche: 5. Oktober 2026, Umsetzung: 18. Oktober 2026. Quellen und Prüfkriterien für Windows 10/11. Der tatsächliche
Funktionsstand je Manager steht in [UNTERSTUETZUNGSMATRIX.md](UNTERSTUETZUNGSMATRIX.md).

## Umsetzungsstand

Der **Systembefund** ist ein gemeinsamer Anwendungsfall von CLI (`system findings [--refresh]`) und Dashboard (Ansicht
**System**). Er prüft ausschließlich Metadaten und erteilt keine Lesefreigabe.

| Teil | Umsetzung | Grenze |
|---|---|---|
| Werkzeuge im Suchpfad | [inventory/system.rs](../src/inventory/system.rs): 23 bekannte Befehle (Node, npm, pnpm, Python, py, pip, uv, Cargo, rustup, .NET, Go, Bun, Yarn, Gradle, Maven, Git, gh, Docker, VS Code, Codex, Claude Code, OpenCode, Gemini) je `PATH`-Eintrag per `symlink_metadata`; Nachweis „Pfad belegt“, „Shim“ (bekannte Shim-Ordner, verknüpfte Suchpfad-Ordner), „App-Alias“ | Höchstens 256 Suchpfad-Einträge und 2 s; relative, `%`-haltige, UNC- und nicht feste Laufwerke werden übersprungen. Nichts wird gestartet: Funktionsfähigkeit und Version bleiben unbekannt |
| Globale Bestände | Bekannte Orte für globale npm-Pakete, uv-Tools (beide Layouts), uv-Python, Cargo-Installationen (`.crates2.json`/`.crates.toml`), rustup, NuGet-Cache, .NET-Tools, pyenv, dazu die Paket-Caches von npm, pip, uv, Cargo (Registry), Gradle, Maven, pnpm und Yarn mit eigener, nur messender Freigabe ([Migration 006](../migrations/006_cache_scopes.sql), Spalte `cache`). Dazu Ablageorte von Werkzeugen (Arbeitsordner von Claude Code unter `%TEMP%\claude`, JetBrains-Scratches unter `%APPDATA%\JetBrains`, `%TEMP%`), je mit eigener Namenssuche | Ein vorhandener Ort ist kein Inhaltsnachweis. Erfassbar nach eigener Freigabe je Ort: npm, uv-Tools, Cargo, NuGet, Scoop, Chocolatey, WinGet. uv-Python, rustup, .NET-Tools und pyenv werden nur als Ort angezeigt; dafür gibt es noch keinen Parser |
| Persönliche KI-Konfiguration | Neuer Freigabetyp `user-ai` ([Migration 004](../migrations/004_user_scopes.sql), Spalte `realm`): nur bekannte Ordner (`.claude`, `.codex`, `.agents`, `.gemini`, `.cursor`, `.continue`, `.roo`, `.config\opencode`, `Code\User`); die Erfassung betritt nur Ordner entlang der Katalogmuster ([profiles](../src/profiles/mod.rs) `user_routes`) | Zugangsdaten, Sitzungen und Verläufe werden nie gelesen; unbekannte Ordner bleiben „nicht zugeordnet“ |
| Scope-Anzeige | KI-Einträge tragen Projekt, Benutzer oder System ([domain/scope.rs](../src/domain/scope.rs) `audience`) | Vorrangauflösung zwischen Benutzer- und Projektebene bleibt offen |
| Zuordnung | Funde verweisen auf eine bestehende Freigabe gleicher Art und gleichen Pfads; Pfade erscheinen mit Platzhaltern wie `%USERPROFILE%`, `%LOCALAPPDATA%` oder `%TEMP%` (auch in dessen 8.3-Form) | Ein Fund im Suchpfad belegt keine Installation durch einen bestimmten Manager |

Messung auf dem Entwicklungsrechner: ein vollständiger Befund in 9,7 ms; die Funde entsprachen der Beobachtungstabelle unten
(einschließlich pyenv-Shim ohne globale Version und älterem uv-Tool-Layout). Die Lesegrenze wurde damit erweitert; nach
[AGENTS.md](../AGENTS.md) braucht das eine zweite Prüfung.

## Ergebnis des Code- und Systemabgleichs (5. Oktober 2026)

Ausgangslage der Umsetzung: Damals gab es statische Projektparser und manuell freizugebende Systemquellen, aber keinen
Systembefund. Die folgenden lokalen Funde zeigten, welche Ergänzungen für diesen Rechner zuerst Nutzen bringen. Die
Erfassungssitzung liefert eine Momentaufnahme, keine vollständige Systemabnahme.

### Tatsächlich beobachtete Ressourcen

Benutzerpfade sind durch Windows-Platzhalter ersetzt. Aus Konfigurationen wurden nur ausgewählte Schlüsselnamen und
Anzahlen ausgewertet; Werte, Credentials und persönliche Projektbezeichnungen werden hier nicht gespeichert.

| Bereich | Beobachtung auf diesem Rechner | Relevanter Erkennungsumfang |
|---|---|---|
| Node / npm | `node`, `npm`, `corepack` im Suchpfad; `%APPDATA%\npm\node_modules` und npm-Cache vorhanden | Runtime, Manager, globale Pakete und Cache getrennt erfassen; Prefix/Config-Overrides beachten |
| Globale npm-Pakete | Paketmetadaten für `@openai/codex` 0.159.2 und `opencode-ai` 1.17.11 vorhanden; daneben ein temporär wirkender OpenCode-Ordner | Paketmetadaten mit Nutzdateien abgleichen; zusätzlichen Ordner als unbestätigten Bestand behandeln |
| uv | `uv`/`uvx` im Suchpfad; `%APPDATA%\uv\tools` enthält eine Umgebung mit `uv-receipt.toml` und `pyvenv.cfg`; uv-Cache vorhanden | Globale Toolumgebungen außerhalb registrierter Projekte erfassen |
| Python / pip / pyenv | `python`, `python3`, `py`, `pip` auffindbar; `python` verweist zuerst auf einen pyenv-Shim. Dessen Aufruf als Prüfrunner scheiterte wegen fehlender globaler/lokaler Versionsauswahl | Launcher, Shim, Auswahl, Interpreter und Paketinstallation getrennt darstellen; die App darf diese Diagnose nicht durch Interpreterstarts nachstellen |
| uv-Python | Drei Verzeichnisse für CPython 3.12.3, 3.12.11 und 3.13.2 mit jeweils `python.exe` unter `%APPDATA%\uv\python` | Tatsächliche Interpreterinstallationsorte erfassen; drei Ordner belegen keine drei aktiven Python-Auswahlen |
| Cargo / rustup | Beide Befehle im Suchpfad; zwei Toolchainordner mit `bin\cargo.exe`, `settings.toml` und Cargo-Registry-Speicher vorhanden | Shims, Toolchains, Cargo-Tools und Quellcache getrennt inventarisieren |
| .NET / NuGet | `dotnet` im Suchpfad; `%APPDATA%\NuGet\NuGet.Config` mit einer Quellenangabe und 101 Paketordner unter `%USERPROFILE%\.nuget\packages` | Globale Konfiguration und gemeinsamen Paketspeicher erfassen; Paketordner sind keine aktuelle Projektzuordnung |
| WinGet | Befehl im Suchpfad auffindbar | Clientnachweis und Windows-Softwareliste unterscheiden; keine Verwaltungszuordnung aus Programmnamen |
| Codex / Agent Skills | `.codex\config.toml`, `AGENTS.md`, `.codex\skills`, `.agents\skills` und Plugin-Cache vorhanden; Konfiguration enthält drei MCP-Einträge sowie `features`, `plugins`, `projects` | Globale Konfiguration, Instruktionen, persönliche/System-Skills, Pluginbestand und deklarierte Aktivierung unterscheiden |
| Claude Code | Befehl, `.claude\settings.json`, persönliche Skills, `.claude.json` und Plugininstallationsdatensatz vorhanden; fünf Plugin-Einstellungen, ein globaler MCP-Eintrag und acht gespeicherte Projektkontexte | Globale Settings/MCP/Plugins erfassen; Projektkontexte sind Referenzen, keine acht aktiven Projekte |
| OpenCode | Befehl und globale npm-Paketmetadaten vorhanden; `.config\opencode` war nicht erreichbar | V1-Profil für den belegten Paketstand verwenden; Konfiguration bleibt ungeprüft |
| VS Code | `code`, Benutzer-`settings.json`, Profile, Snippets, Extensions, `workspaceStorage` und `Backups` vorhanden | Globale Settings und Extension-Manifeste erfassen; Arbeitszustand und ungespeicherte Inhalte geschützt halten |
| Git | `git` und `.gitconfig` mit Benutzersektion vorhanden | Globale Konfigurationsquelle zeigen; Benutzername und E-Mail nicht übernehmen |
| Weitere Dev-Werkzeuge | `gh` und `docker` im Suchpfad; GitHub-CLI-Authentifizierungsdatei und `.wslconfig` vorhanden; Docker-Konfigurationsorte nicht erreichbar | Spätere Profile; Authentifizierungsdateien höchstens als geschützte Orte anzeigen |
| Arbeitsordner / Scratchpads | Ein Codex-Worktree-Kandidat mit `.git` als Datei; VS-Code-Backupbereich vorhanden. Kein Scratchpad an den geprüften konventionellen Scratch-/Sandbox-Orten bestätigt | Arbeitskopie und Editor-Backup unterscheiden; Scratchpad-Zweck und Besitzer bleiben offen |

Die Standarddateien `.npmrc`, Cargo-`config(.toml)`/Installationsdatensätze, Benutzer-pip-`pip.ini` und Benutzer-uv-`uv.toml`
wurden an den geprüften Orten nicht gefunden. Das schließt Konfiguration über andere Pfade, Umgebungen oder Laufzeitflags
nicht aus. pnpm, Scoop und Chocolatey wurden im geprüften Suchpfad/Standardumfang nicht belegt und bleiben im Katalog als
weitere Erkennungskandidaten relevant. Kein negativer Teilbefund wird als vollständiger Abwesenheitsnachweis gewertet.

**Versionsunterschied bei uv:** Die aktuelle [Storage-Dokumentation](https://docs.astral.sh/uv/reference/storage/) nennt
`%APPDATA%\uv\data\tools` und `...\data\python`. Hier liegen die beobachteten Bestände direkt unter `uv\tools` und
`uv\python`; die beiden `data`-Kandidaten wurden nicht gefunden. Die Erkennung muss belegte bestehende Orte und
versionsabhängige Kandidaten berücksichtigen. Eine Abweichung vom neuesten Default belegt keine verwaiste Installation.

### Was im Code fehlte (Stand 5. Oktober; Umsetzung siehe oben)

| Anwendungsfall | Damaliger Code | Fehlende Funktion |
|---|---|---|
| Welche Manager liegen auf dem System? | [desktop/view.rs](../src/desktop/view.rs) baut die Ansicht aus allen Adapterdeskriptoren; [CLI commands](../src/bin/cli/commands.rs) liefert für `adapters` ebenfalls den Katalog | Eigener Systembefund mit Installationsnachweis, Shim-/Aliasstatus, Quelle und Prüfzeit |
| Globale Dev-/KI-Konfiguration | [ScopeKind](../src/domain/scope.rs) und [Schema v2](../migrations/002_inventory.sql) kennen nur Projekt, Scoop, Chocolatey und Registry; [service](../src/service/mod.rs) bietet nur diese Freigaben | Enge globale Konfigurationsbereiche mit eigenen Selektoren und Benutzerscope, ohne das gesamte Home als Projekt freizugeben |
| Globale Pakete, Tools, Stores | [inventory](../src/inventory/mod.rs) dispatcht ausschließlich Projekt-, Scoop-/Chocolatey- und Registry-Erfassung; [Adapter](../src/adapters/mod.rs) erhalten nur diesen Kontext | Globale npm-, uv-, Cargo- und NuGet-Quellen sowie Interpreter-/Toolchainnachweise |
| Existierende Scratchpads finden | [service](../src/service/mod.rs) und [storage/resources](../src/storage/resources.rs) implementierten Anlegen (am 20.10.2026 entfernt) und explizites Registrieren/Übernehmen | Begrenzte Suche nach Arbeitsordnern innerhalb eigener freigegebener Suchwurzeln; Kandidaten mit Evidenz statt automatischer Übernahme |
| Scope korrekt anzeigen | [service/snapshot.rs](../src/service/snapshot.rs) kennzeichnet KI-Einträge derzeit fest als `Projekt` | Globaler Benutzer-, System- und Projektbezug sowie unvollständige Vorrangauflösung getrennt anzeigen |

Der [KI-Katalog](../profiles/ai-catalog.toml) verwendet relative Projektmuster. Eine schmale Freigabe nur von `.codex`
würde zum Beispiel `config.toml` liefern, während das bestehende Muster `.codex/config.toml` erwartet. Neue globale Orte
benötigen passende Selektoren und Scopekontext; zusätzliche Pfadstrings allein schließen diese Lücke nicht.

### Priorität für diesen Rechner

1. **Umgesetzt:** gemeinsamer Service-Anwendungsfall für Systembefunde und gezielte Datenortvorschläge; CLI und Dashboard
   zeigen dieselben Nachweise. PATH-/Known-Folder-Prüfung ersetzt keine Inhaltsfreigabe.
2. **Weitgehend umgesetzt:** globale npm-/uv-Tool-/Cargo-/NuGet-Bestände sowie Codex-/Agent-Skills-/Claude-/OpenCode-/
   VS-Code-Quellen mit eigenen Lesebereichen. Offen: uv-Python, rustup, .NET-Tools und pyenv als erfasste Bestände (bisher
   nur Ortsnachweis) sowie die Git-Konfiguration.
3. **Teilweise umgesetzt:** Projektsuchwurzeln (`system approve --kind projects`) liefern Kandidaten. Offen: Scratchpad-Suche;
   `scratch_root` bleibt der Erstellungsort.
4. Zusätzliche Manager und Entwicklerprofile bei belegten Funden erweitern. Ein unterstützter Parser ist keine Aussage,
   dass das zugehörige Werkzeug installiert ist.

Grenzen dieses Abgleichs: keine vollständige Laufwerkssuche und keine persönlichen Scratchpad-Wurzeln benannt. Der
Standard-CLI-Zugang meldete mit `--no-start` keinen erreichbaren Besitzer; der Datenbankbestand wurde deshalb nicht direkt
gelesen oder verändert. Ein laufender mogumogu-Prozess allein beweist weder den lokalen Modus noch dieselbe Datenwurzel.

## Ziel und Evidenz

Für den gewünschten Anwendungsfall sind drei Ebenen relevant: vorhandene Werkzeuge und globale Pakete, globale Entwicklungs-
und Agentenkonfiguration, sowie bestehende Arbeitsordner einschließlich möglicher Scratchpads.

Die Anwendung sollte je Fund getrennt ausweisen:

| Aussage | Erforderliche Evidenz | Aussagegrenze |
|---|---|---|
| Werkzeugpfad vorhanden | vorhandene Datei aus PATH, Installationsort oder OS-Metadaten | Kann ein Alias, Shim oder verwaister Launcher sein |
| Installation statisch belegt | passendes Installationsmanifest oder Paketmetadaten plus vorhandene Nutzdateien | Keine Aussage über erfolgreiche Ausführung |
| Konfiguration vorhanden | bekannte Datei an einem aufgelösten Ort | Kein Installations- oder Wirksamkeitsnachweis |
| Paket deklariert / installiert / im Cache | jeweils Manifest / Installationsmetadaten / Speicherstruktur | Diese Zustände sind unabhängig |
| Im aktuellen Projekt wirksam | bekannte Scope- und Vorrangregeln, alle relevanten Ebenen erfasst | Laufzeitflags und Remote-Policy können unbekannt bleiben |
| Scratchpad-Kandidat | begrenzter Suchraum plus Arbeitsordner-Evidenz | Zweck, Besitz und Entbehrlichkeit bleiben unbestätigt |

Ein `npx`-/`uvx`-Eintrag ist eine Ausführungsreferenz. Ein vorhandener Cache, ein Verzeichnisname oder ein registrierter
Projekteintrag genügt nicht als Nachweis einer vollständigen Installation oder eines entbehrlichen Arbeitsordners. Die
Tabelle legt Produktkriterien fest; sie ist kein Herstellerstandard.

## Paketmanager, Interpreter und globale Pakete

### npm / Node.js

- Globale Windows-Kandidaten: `%APPDATA%\npm`, Pakete unter `<prefix>\node_modules`, Launcher direkt unter `<prefix>`, Cache
  unter `%LOCALAPPDATA%\npm-cache`. Der tatsächliche Prefix kann abweichen. [npm:
  Folders](https://docs.npmjs.com/cli/v11/configuring-npm/folders/)
- Konfiguration: Benutzer-`.npmrc`, `<prefix>\etc\npmrc` und die mit npm ausgelieferte `npmrc`. Overrides:
  `NPM_CONFIG_USERCONFIG`, `NPM_CONFIG_GLOBALCONFIG`, `NPM_CONFIG_PREFIX`, `NPM_CONFIG_CACHE`. [npm:
  Config](https://docs.npmjs.com/cli/v11/using-npm/config/)
- Prüfkriterium: `node.exe` und npm-Paketmetadaten getrennt prüfen; globale Pakete anhand ihres `package.json` und
  vorhandener Nutzdateien erfassen. Scoped-Pakete mitnehmen; Cache- und temporäre Installationsreste separat halten.
- Schutz: `.npmrc` nur über erlaubte Struktur- und Pfadfelder auswerten; Authentifizierung, private Registry-Adressen und
  Zertifikatschlüssel ausschließen.

### uv

- Konfiguration: `%APPDATA%\uv\uv.toml`, `%PROGRAMDATA%\uv\uv.toml`; Projektkonfiguration ist für `uv tool` nicht maßgeblich.
  [uv: Configuration files](https://docs.astral.sh/uv/concepts/configuration-files/)
- Persistente Daten: `%APPDATA%\uv\data\tools` und `...\python`, Cache: `%LOCALAPPDATA%\uv\cache`. Overrides: `UV_TOOL_DIR`,
  `UV_TOOL_BIN_DIR`, `UV_PYTHON_INSTALL_DIR`, `UV_PYTHON_BIN_DIR`, `UV_INSTALL_DIR`; `UV_CONFIG_FILE` und `UV_CACHE_DIR`
  gesondert berücksichtigen. [uv: Storage](https://docs.astral.sh/uv/reference/storage/), [uv: Environment
  variables](https://docs.astral.sh/uv/reference/environment/)
- Prüfkriterium: `uv.exe`/`uvx.exe`, verwaltete Python-Installationen und installierte Tools getrennt erfassen.
  Toolverzeichnisse mit `uv-receipt.toml`, Umgebungsmetadaten und Nutzdateien abgleichen. [uv: Toolverwaltung im
  Quellcode](https://github.com/astral-sh/uv/blob/main/crates/uv-tool/src/lib.rs)
- Schutz: Credentials-Speicher, `.env`-Dateien, Index-Zugangsdaten und Download-/Buildinhalte nicht als globale
  Konfigurationsinventare übernehmen.

### Python / pip / Interpreter-Shims

- pip-Konfiguration: `%APPDATA%\pip\pip.ini`, ältere `%HOME%\pip\pip.ini`, systemweit `C:\ProgramData\pip\pip.ini`; je
  Umgebung deren `pip.ini`. `PIP_CONFIG_FILE` überschreibt die Dateiauswahl, weitere `PIP_*`-Variablen können Einstellungen
  übersteuern. [pip: Configuration](https://pip.pypa.io/en/stable/topics/configuration/)
- Benutzerpakete verwenden auf Windows das versionsabhängige Schema `%APPDATA%\Python\PythonXY\site-packages`;
  `PYTHONUSERBASE` kann die Basis verändern. Installations- und venv-Pakete benötigen eigene Wurzeln. [Python:
  site](https://docs.python.org/3/library/site.html)
- Prüfkriterium: Python-Launcher, Interpreterdatei, Shim, Interpreterauswahl und `pip`-Distribution getrennt erfassen.
  Vorhandene `*.dist-info`-Metadaten belegen Pakete; eine `pip.exe` oder ein PATH-Name allein belegt keinen funktionsfähigen
  Interpreter. Keine Interpreter für die Erfassung starten.
- pyenv-win ergänzt die globale Auswahl `.pyenv\pyenv-win\.python-version` und Interpreter unter
  `.pyenv\pyenv-win\versions\<version>\python.exe` im Benutzerprofil. Auswahl und vorhandene Binärdatei abgleichen.
  [pyenv-win: Offizielle README](https://github.com/pyenv-win/pyenv-win/blob/master/README.md)
- Schutz: `.pth`, `sitecustomize`, `usercustomize` und Aktivierungsskripte bleiben Daten und werden nicht ausgeführt;
  Index-URLs können Secrets enthalten.

### Cargo / rustup

- Cargo-Home: `%USERPROFILE%\.cargo` beziehungsweise `CARGO_HOME`; globale `config.toml`, Installationsmetadaten
  `.crates.toml`/`.crates2.json`, `bin`, Registry- und Git-Speicher getrennt erfassen. `bin` enthält auch rustup-Werkzeuge;
  es ist keine Liste ausschließlich installierter Crates. [Cargo: Cargo
  Home](https://doc.rust-lang.org/cargo/guide/cargo-home.html)
- rustup-Home: `%USERPROFILE%\.rustup` beziehungsweise `RUSTUP_HOME`, mit `settings.toml` und Toolchains; `RUSTUP_TOOLCHAIN`
  kann die Auswahl ändern. Das interne Settings-Schema ist ausdrücklich keine stabile öffentliche API. [rustup:
  Configuration](https://rust-lang.github.io/rustup/configuration.html), [rustup: Environment
  variables](https://rust-lang.github.io/rustup/environment-variables.html)
- Prüfkriterium: rustup-Shims, installierte Toolchains und Cargo-Tools mit Installationsmetadaten abgleichen.
  Registry-Quellen sind Cachebestände.
- Schutz: `credentials.toml` und ältere Credential-Dateien nicht lesen; private Registry-/Git-Adressen sowie
  Compiler-/Wrapperbefehle nicht ausführen.

### NuGet / .NET

- Globale Konfiguration: `%APPDATA%\NuGet\NuGet.Config`, zusätzliche `%APPDATA%\NuGet\config\*.Config`, systemweit
  `%ProgramFiles(x86)%\NuGet\Config`; projektabhängig weitere Ebenen. [NuGet: Common
  configurations](https://learn.microsoft.com/en-us/nuget/consume-packages/configuring-nuget-behavior)
- Paketspeicher: `%USERPROFILE%\.nuget\packages`; Overrides: `NUGET_PACKAGES`, `globalPackagesFolder`, `repositoryPath` und
  bei MSBuild `RestorePackagesPath`. Ein Speicherfund belegt keine aktuelle Projektverwendung. [NuGet: Global packages and
  cache](https://learn.microsoft.com/en-us/nuget/consume-packages/managing-the-global-packages-and-cache-folders)
- Globale .NET-Tools: `%USERPROFILE%\.dotnet\tools` mit Nutzdateien in `.store`; `--tool-path` erlaubt beliebige andere Orte.
  Lokale Tools verwenden `.config\dotnet-tools.json` und den NuGet-Speicher. [Microsoft: dotnet tool
  install](https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-tool-install)
- Prüfkriterium: `dotnet.exe`, SDK-/Runtime-Dateien, NuGet-Paketbestand und Toolinstallationen getrennt belegen. Credentials
  und private Quellen redigieren; MSBuild-/Projektdateien ausschließlich statisch lesen.

## Globale Entwicklungs- und Agentenkonfiguration

### Git und VS Code

- Git: `%USERPROFILE%\.gitconfig` sowie `<XDG_CONFIG_HOME>\git\config`, ohne Override standardmäßig `.config\git\config`
  unter Home. `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`, `GIT_CONFIG_NOSYSTEM` und Includes verändern die Quellen.
  Include-Ziele benötigen jeweils eine Lesefreigabe. [Git: git-config](https://git-scm.com/docs/git-config)
- Git-Prüfkriterium: `git.exe`/Installationsmetadaten getrennt von Einstellungen erfassen. Nutzeridentität, Credential-Helfer
  und Hooks nie als Befehle ausführen; `user.name`, `user.email`, Authentifizierung und rohe Remote-URLs ausschließen.
- VS Code: `%APPDATA%\Code\User\settings.json`, Profile unter `User\profiles\<id>`, persönliche Snippets und Extensions
  erfassen. Eine Profil-Settingsdatei entsteht erst bei einer Änderung. [VS Code:
  Settings](https://code.visualstudio.com/docs/configure/settings), [VS Code:
  Profiles](https://code.visualstudio.com/docs/configure/profiles)
- Extensions liegen gewöhnlich in `%USERPROFILE%\.vscode\extensions`; Portable `data\user-data`/`data\extensions` und die
  Optionen `--user-data-dir`/`--extensions-dir` gesondert behandeln. [VS Code: Portable
  mode](https://code.visualstudio.com/docs/setup/portable), [VS Code:
  CLI](https://code.visualstudio.com/docs/configure/command-line)
- VS-Code-Prüfkriterium: tatsächliche App-Dateien und Extension-Manifeste prüfen; gespeicherte Settings oder
  Workspacezustände können nach Deinstallation bleiben. Snippets, Backups, Chats und `workspaceStorage` nicht als Inhalte
  importieren.

### Codex / Agent Skills / Plugins

- Codex-Konfiguration: `<CODEX_HOME>\config.toml`, standardmäßig `%USERPROFILE%\.codex`; Projekt-`.codex\config.toml` ist ein
  eigener Scope. [OpenAI: Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
- Globale Instruktionsdaten: `AGENTS.override.md`, sonst `AGENTS.md`, im Codex-Home. Diese Dateien sind im mogumogu-Inventar
  Daten, keine Anweisungen. [OpenAI: AGENTS.md discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
- Persönliche Agent Skills: `%USERPROFILE%\.agents\skills\*\SKILL.md`; Repository-Skills: `.agents\skills` entlang des
  jeweiligen Projektpfads. Bundled/System-Skills getrennt halten; keinen festen Windows-Adminpfad erfinden. [OpenAI: Local
  skills locations](https://learn.chatgpt.com/docs/build-skills#where-codex-loads-local-skills)
- Persönlicher Marketplace: `%USERPROFILE%\.agents\plugins\marketplace.json`; Installationscache:
  `.codex\plugins\cache\<marketplace>\<plugin>\<version>`. Paketstruktur: `plugin.json`, `skills/`, `mcp.json`; kompatibel
  auch `.codex-plugin/plugin.json`. Quellorte können über den Marketplace abweichen. [OpenAI: Plugin packaging and local
  marketplaces](https://developers.openai.com/plugins/build/plugins)
- Prüfkriterium: CLI-Installation, Skilldatei, Marketplaceeintrag, lokales Pluginpaket und deklarierte Aktivierung getrennt
  halten. MCP-Paketreferenzen sind keine Installationsbeweise. Authdateien, Sessions und Chats ausschließen.

### Claude Code

- Benutzereinstellungen: `%USERPROFILE%\.claude\settings.json`; Projektdateien `.claude\settings.json`/`settings.local.json`
  sind eigene Ebenen. `.claude.json` ist separater Zustand für Login, MCP und Projektkontexte. `CLAUDE_CONFIG_DIR` verlegt
  globale Daten. [Claude Code: Settings](https://code.claude.com/docs/en/settings)
- Persönliche Skills: `.claude\skills\*\SKILL.md` unter Home; Plugin-Skills liegen unter `<plugin>\skills`. Synchronisierte
  Skills und ältere `.claude\commands` separat klassifizieren. [Claude Code: Skills](https://code.claude.com/docs/en/skills)
- Pluginzustand: `<Claude-Home>\plugins\installed_plugins.json`, `known_marketplaces.json`,
  `cache\<marketplace>\<plugin>\<version>` und persistente `data` getrennt erfassen. `CLAUDE_CODE_PLUGIN_CACHE_DIR` kann die
  Pluginwurzel verlegen. `enabledPlugins` ist deklarierte Aktivierung; die tatsächlich geladene Session bleibt eine andere
  Stufe. [Claude Code: Plugin loading](https://code.claude.com/docs/en/plugins/loading)
- Prüfkriterium: Settings, Skills, Installation und deklarierte Aktivierung getrennt anzeigen. `.claude.json` nur mit enger
  Feld-Allowlist auswerten; Login, Credentials, Sessionverläufe, Hooks, Helfer und MCP niemals starten.

### OpenCode

- Globale Kandidaten: `%USERPROFILE%\.config\opencode\opencode.json(c)`, Konfigverzeichnisse für `agents`, `commands`,
  `plugins`, `skills`; zusätzlich `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, `OPENCODE_CONFIG_CONTENT` beachten.
  Systemkonfiguration kann unter `%ProgramData%\opencode` liegen. [OpenCode: Config](https://opencode.ai/docs/config/)
- Für belegte V1-Installationen `tui.json` priorisieren. V2 globale `cli.json` beziehungsweise
  `<XDG_CONFIG_HOME>\opencode\cli.json` und `OPENCODE_CLI_CONFIG_CONTENT` bleiben gesonderte Versionskandidaten.
  [OpenCode V2: CLI settings](https://opencode.ai/v2/docs/cli/config)
- Schutz: `.local\share\opencode\auth.json` enthält Authentifizierung; Projekt-/Sessiondaten und Logs liegen getrennt von
  Konfiguration. [OpenCode: Storage and troubleshooting](https://opencode.ai/docs/troubleshooting/)
- Prüfkriterium: App-/Paketinstallation getrennt von Konfigdateien belegen. Nicht erreichbare Kandidaten bedeuten „nicht
  geprüft“, keine Abwesenheit. Remote-Organisationskonfiguration nicht durch die Erfassung herunterladen.

## Weitere Kandidaten bei Bedarf

- pnpm: aktuelle globale `config.yaml`/`rc` unter `%LOCALAPPDATA%\pnpm\config`, beziehungsweise über `XDG_CONFIG_HOME`;
  Format/Scope anhand der erkannten Version wählen. Authdaten separat schützen. [pnpm: config](https://pnpm.io/cli/config)
- Scoop: `%USERPROFILE%\scoop`, `%PROGRAMDATA%\scoop`, Overrides `SCOOP`, `SCOOP_GLOBAL`, `SCOOP_CACHE`; Konfiguration über
  `XDG_CONFIG_HOME\scoop\config.json` oder Home-`.config`. [Scoop: Installer
  source](https://github.com/ScoopInstaller/Install/blob/master/install.ps1)
- Chocolatey: `%PROGRAMDATA%\Chocolatey` oder `ChocolateyInstall`; Paketdatensätze und tatsächliche installierte Programme
  getrennt erfassen. [Chocolatey: Setup](https://docs.chocolatey.org/en-us/choco/setup/)
- WinGet: App-Installer-Installation, Clientsettings und OS-Softwareinventar getrennt erfassen; ein Softwareeintrag belegt
  keine Verwaltung durch WinGet. Den Settings-CLI-Aufruf nicht zur statischen Erfassung verwenden: Er öffnet einen Editor.
  [Microsoft: winget settings](https://learn.microsoft.com/en-us/windows/package-manager/winget/settings)

Diese Liste behauptet keine lokalen Funde. Weitere globale Entwicklerprofile wie Docker, PowerShell, SSH, GitHub CLI und WSL
folgen derselben Evidenztrennung.

## Bestehende Scratchpads und Suchwurzeln

**Umsetzung vom 18. Oktober:** Die [Fundliste](ENTWICKLUNGSFUNDE.md) erkennt jetzt Entwicklungskandidaten anhand von Namen und Metadaten in freigegebenen Such- und Projektwurzeln. Ausdrückliche Zuordnung und Zweckangabe sind implementiert; automatische Zweckbestimmung, vollständige Worktree-Auflösung und Editor-Backuperkennung bleiben offen. Die folgenden Punkte beschreiben weitergehende Zielkriterien.

Die untersuchten Quellen definieren keinen gemeinsamen Scratchpad-Standard. „scratch“, „tmp“, „sandbox“ oder fehlendes Git
sind höchstens Suchhinweise. Die Anwendung muss vorhandene Ordner zunächst als Arbeitsordner-Kandidaten zeigen; Zuordnung und
Umgang mit ihnen brauchen weitere Evidenz.

- Suchwurzeln: bereits freigegebene Projekt-/Arbeitswurzeln und ausdrücklich ausgewählte persönliche Entwicklungsordner.
  Keine rekursive Vollsuche über Home, alle Laufwerke oder sämtliche temporären Dateien.
- Projektkandidaten: Manifeste, Lockfiles, `.git`-Datei/-Verzeichnis, venv-Marker und bekannte Instruktionsdateien innerhalb
  dieser Wurzeln. Ein Manifest belegt einen Projektbezug, keinen temporären Zweck.
- Git-Worktrees: getrennte Arbeitskopien mit gemeinsamem Repositoryzustand; als Arbeitsordner erkennen, nicht automatisch als
  Wegwerf-Scratchpads. [OpenAI: Git worktrees](https://learn.chatgpt.com/docs/environments/git-worktrees)
- Claude hat produktspezifische Scratchdateien unter `<CLAUDE_JOB_DIR>\tmp`. Diese Sessionbereiche sind keine universellen
  Entwickler-Scratchpads und können sensible Inhalte enthalten. [Claude Code: Environment
  variables](https://code.claude.com/docs/en/env-vars)
- Editor-/Agent-Projektlisten können veraltete Referenzen enthalten. Referenz vorhanden, Ziel vorhanden, Ziel lesbar und
  Projekt erkannt bleiben getrennte Zustände; Workspace-State-Ordner nicht als Projektanzahl zählen.
- Von mogumogu selbst verwaltete Scratchpads brauchen einen eigenen überprüfbaren Besitzmarker. Ein Fremdordner darf nicht
  allein durch Registrierung oder Namensähnlichkeit diesen Besitzstatus erhalten.

## Anforderungen an die spätere Erfassung

1. PATH-, Known-Folder- und erlaubte Pfad-Override-Metadaten sammeln; vollständige Umgebungsvariablen oder Werte anderer
  Konfiguration nicht sichern.
2. Kandidaten und Befunde mit Quelle, Scope, Zeit, Erreichbarkeit und Unsicherheit getrennt speichern. Zugriffsfehler nie zu
  „fehlt“ umdeuten.
3. Registrierung und Lesefreigabe unterscheiden; Inhalte ausschließlich unter identitätsgebundener Freigabe über die
  bestehende Lesegrenze prüfen.
4. Paket-/Toolmetadaten begrenzt und statisch parsen. Reparse Points, Includes und verlegte Wurzeln brauchen dieselben
  Sicherheitsprüfungen.
5. Installation, globale Konfiguration, Projektabhängigkeit, Cache und Arbeitsordner in der Anzeige als unterschiedliche
  Befunde behandeln.
6. Keine Shellaufrufe, Interpreterstarts, Hooks, Paketmanager-Kommandos oder Netzwerkzugriffe beim Erfassen. Funktionsfähigkeit bleibt
  ohne separaten Test unbekannt.
7. Für zusätzliche Lesegrenzen zweiten Review und passende Windows-Fixtures vorsehen. Automatische Identifikation erteilt
  keine Bereinigungserlaubnis.

Offene Doku-/Versionsgrenzen: npm-Prefix hängt von Installation/Overrides ab; Cargo-/rustup-Interna sind nicht stabil;
Codex-Bundles sind nicht auf einen allgemeinen System-Skillpfad festgelegt; OpenCode V1/V2 benötigen Zuordnung; umgeleitete
Installationen und unbekannte Suchwurzeln begrenzen Vollständigkeit.

## Abnahmekriterien für die fehlende Funktion

- Ein frisches lokales Inventar zeigt die auf diesem Testsystem belegten Werkzeuge und Datenortvorschläge automatisch,
  auch bevor ein Projekt registriert wurde. Katalogfähigkeiten und Systembefunde sind getrennte Angaben.
- Der pyenv-Shim wird als Launcher mit ungeprüfter Interpreterauswahl dargestellt; vorhandene uv-Python-Binärdateien
  werden gesondert erkannt, ohne den Shim oder einen Interpreter auszuführen.
- Nach gezielter Lesefreigabe erscheinen globale Paket-/Toolmetadaten und Konfigurationsbefunde mit richtigem Scope;
  insbesondere werden die hier beobachteten uv-Datenorte nicht durch neuere Defaultpfade übergangen.
- Arbeitsordner innerhalb freigegebener Suchwurzeln erscheinen mit Manifest-/Worktree-/Umgebungsevidenz und unbekanntem
  Besitzer, solange keine Zuordnung bestätigt ist. Backupbereiche werden nicht als Scratchpads übernommen.
- Nicht erreichbare, verlegte, ersetzte oder verlinkte Quellen ergeben nachvollziehbar ungeprüfte/partielle Abdeckung.
  Wiederholte Scans erzeugen weder Dubletten noch Entfernungsmeldungen aus einem Teillauf.
- Synthetische Geheimnisse und fremde Instruktionen in globalen Mischdateien gelangen nicht als Werte in Datenbank,
  Logs, Diffs oder Export. Enge Root-Selektoren funktionieren ohne pauschale Home-Freigabe.

Diese Kriterien sind noch nicht durch eine implementierte Erkennung erfüllt. Dieser Prüfstand ändert Dokumentation,
keine Scanner-, Lese-, Datenbank- oder Bereinigungslogik.
