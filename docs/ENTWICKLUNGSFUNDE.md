# Entwicklungsfunde, Speicher und Skill-Inhalte

Stand: 30. Oktober 2026. Die neuen Fähigkeiten sind experimentell. Tests verwenden
synthetische Windows-Verzeichnisse; eine Windows-11-Abnahme steht aus.

## Fundliste

Die Dateinamensuche (`projects`) und freigegebene Projektbereiche erkennen
Projektmarker, mögliche Scratchpads, einzelne Skripte ohne Projektmarker,
Notebooks, Testordner, Python-Umgebungen, Abhängigkeitsordner, typische
Build-/Testausgaben, Testdaten, lokale Datenbanken, bekannte Cache-Namen und
Ordner mit `CACHEDIR.TAG`, deren Inhalt nicht betreten wird. Diese Erkennung
liest nur Verzeichniseinträge. Große erkannte Speicher und Ausgabeordner werden
als Funde erfasst und in der Suche nicht weiter betreten. Die Paketadapter
können nach Projektfreigabe eng begrenzte Manifest-/Installationsmetadaten
lesen.

Dieselbe Regel gilt in der Projekterfassung: Ordner, die die Erkennung als
Blatt einstuft (Umgebungen wie `.venv` auch ohne `pyvenv.cfg`, Ausgaben wie
`dist`, `out`, `build`, mitgelieferter Fremdcode wie `vendor`, `third_party`,
`deps`, Test-Ausgaben wie `coverage`, Caches), erscheinen als Fund, werden aber
nicht weiter betreten; ihre Manifeste zählen nicht als eigene des Projekts.
Ausnahme ist ein Ordner, den ein Paketadapter beansprucht (etwa `node_modules`,
`vendor` neben `composer.json`, `Pods` neben dem `Podfile`); der Adapter liest
ihn eng begrenzt. Die freigegebene Wurzel selbst wird immer durchlaufen, auch
wenn sie `build` heißt.

Jeder Fund zeigt Typ, Beleg, Pfad, Scanabdeckung, mögliche Projektzuordnung und
bekannte Dateigröße/Änderungszeit. Ein Änderungsdatum ist kein Nutzungsnachweis.
Ordnernamen belegen weder Testzweck noch Entbehrlichkeit. Unbekannte Inhalte
bleiben unbekannt; Editor-Backups werden nicht als Scratchpads übernommen.

### Projekte

Ein Projekt erkennt mogumogu an Manifesten (`Cargo.toml`, `package.json`,
`pyproject.toml`, `setup.py`, `pixi.toml`, `go.mod`/`go.work`, `pom.xml`,
Gradle-Build- und -Settings-Dateien, `build.sbt`, `deps.edn`, `project.clj`,
`composer.json`, `Gemfile`, `mix.exs`, `rebar.config`, `gleam.toml`,
`pubspec.yaml`, `deno.json(c)`, `Package.swift`, `Podfile`, `Cartfile`,
`build.zig`/`build.zig.zon`, `stack.yaml`/`cabal.project`/`*.cabal`,
`Project.toml`, `shard.yml`, `elm.json`, `dub.json`/`dub.sdl`, `dune-project`,
`cpanfile`, `Makefile.PL`, `vcpkg.json`, `conanfile.txt`/`conanfile.py`,
`meson.build`, `MODULE.bazel`/`WORKSPACE`, `pants.toml`, `rush.json`,
`platformio.ini`, `project.godot`, `flake.nix`, Projektdateien wie
`.csproj`/`.sln`/`.vcxproj`/`.sqlproj`/`.pyproj`, `*.gemspec`, `*.podspec`,
`*.nimble`, `*.opam`, `*.rockspec`, `*.dproj`, `*.lpi`, `*.uproject`), an
Xcode-Bündeln (`*.xcodeproj`, `*.xcworkspace`), an Unity-Projekten (`Assets`
neben `ProjectSettings`), an `.git`/`.hg`/`.svn` und an schwächeren Markern
(`requirements.txt`, `setup.cfg`, `Pipfile`, `uv.lock`, `tox.ini`,
`noxfile.py`, `manage.py`, `environment.yml`, `Makefile`, `justfile`,
`Taskfile.yml`, `CMakeLists.txt`, `CMakePresets.json`, `configure.ac`,
`xmake.lua`, `premake5.lua`, `packages.config`, `Directory.Build.props`,
`global.json`, `tsconfig.json`, `lerna.json`, `nx.json`, `turbo.json`,
`melos.yaml`, `.tool-versions`, `mise.toml`, `devbox.json`, `Dockerfile`,
Compose-Dateien, `Chart.yaml`, `Pulumi.yaml`, `wrangler.toml`, `mkdocs.yml`,
`*.code-workspace`, `*.Rproj`, `*.tf`). Jeder Projektfund trägt eine Sicherheit:

| Sicherheit | Namen im Projektordner |
|---|---|
| hoch | Versionsverwaltung und Manifest, Versionsverwaltung mit eigener CI (etwa CMake- oder Make-Projekte), oder Manifest mit Lockfile oder Quellordner |
| mittel | nur Manifest, oder Versionsverwaltung ohne Manifest und CI |
| gering | nur schwache Marker; außerdem jeder Ordner mit dem Namen eines Archiv-Downloads ohne Versionsverwaltung (`*-main`, `*-master`, `name-1.4.2`) |

Liegt ein Projekt unter `Downloads`, ist die Sicherheit höchstens mittel. Bei
gleicher Sicherheit stehen Teilprojekte nach den äußeren Projekten. Daneben
nennt der Fund Hinweise, in dieser Reihenfolge:

- „Paketmanager: …“ nach Dateien, die nur ein Manager schreibt oder liest
  (`pnpm-lock.yaml` → pnpm, `poetry.lock` → Poetry, `Gemfile` → Bundler,
  `*.csproj` → NuGet, `*.tf` → Terraform …), höchstens fünf, dann „+N“; eine
  bloße `package.json` nennt keinen Manager;
- „Workspace (…)“ für Werkzeuge mehrteiliger Projekte (`pnpm-workspace.yaml`,
  Lerna, Nx, Turborepo, Rush, `go.work`, Gradle-Settings, Cabal, Melos, Bazel,
  Pants);
- „Tests (N Testdateien · Frameworks)“ aus der Zählung unten, „Tests“ allein,
  wenn ein Testordner direkt im Projekt liegt, aber nichts gezählt wurde;
- „Testprojekt“ für eine .NET-Projektdatei mit Testnamen (`App.Tests.csproj`,
  `AppUnitTests.fsproj`, `App.Specs.csproj`);
- Versionsverwaltung, Lockfile, Quellcode, CI (`.github`, `.gitlab-ci.yml` …),
  KI-Dateien (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.claude`, `.cursor`,
  `.codex` …) und Editor-Einstellungen.

Die Hinweise stammen nur aus Dateinamen; kein Manifest wird dafür gelesen. Ein
Projekt in einem anderen Projekt derselben Suche ist ein **Teilprojekt**; sein
Projektbezug zeigt auf das äußere Projekt, und der Filter **Projekte** zeigt nur
die äußeren. Innerhalb eines Projekts machen `tsconfig.json` und
`Directory.Build.props` kein Teilprojekt, ebenso wenig `CMakeLists.txt`,
`Makefile`, `meson.build` oder nur schwache Marker in einem Testordner; ein
Xcode-Bündel gehört zum Projekt um es herum. Bei einer Laufwerkssuche machen
`.tool-versions`, `mise.toml` und `.mise.toml` an Laufwerks- oder Profilwurzel
kein Projekt. Mitgelieferter Fremdcode (`vendor`, `third_party`,
`bower_components`, `Pods`, `deps`, `vcpkg_installed`, `.terraform` …) ist in
einem Projekt ein Abhängigkeitsfund und wird nicht betreten; Unitys `Library`
mit `PackageCache` oder `ScriptAssemblies` ist ein Cache-Fund, damit ihre Pakete
nicht als Teilprojekte erscheinen. Als Aktivität zeigt der Fund das neueste
Änderungsdatum der Einträge des Ordners, auf den Tag gerundet. Sicherheit und
Datum ordnen die Liste; sie belegen weder Nutzung noch Entbehrlichkeit.

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

### Tests

Ein Ordner mit testtypischem Namen (`tests`, `test`, `__tests__`, `spec`,
`specs`, `e2e`, `e2e-tests`, `integration-tests`, `integration_tests`,
`integration_test`, `unittests`, `unit_tests`, `unit-tests`, `androidTest`,
`cypress`; `testing` nur in einem Projekt) wird als Fund **Tests** gemeldet,
wenn er selbst keinen Projektmarker trägt, nicht in Testdaten liegt und
entweder in einem Projekt liegt oder selbst Skripte, Testdateien oder Ordner
wie `.venv` oder `node_modules` enthält. Gemeldet wird nur der äußerste
Testordner; dazu zählen auch Mavens `src/test` und SwiftPMs `Tests`. Der Fund
ist kein Blatt: Die Suche läuft weiter hinein, um zu zählen.

Gezählt werden Testdateien nach ihren Namen, in einem Testordner zusätzlich
jede Quelldatei:

| Sprache | Testdateien |
|---|---|
| Python | `test_*.py`, `*_test.py`, `tests.py`, `conftest.py` (pytest) |
| JavaScript/TypeScript | `*.test.*`, `*.spec.*`, `*.e2e-spec.*`, `*.cy.*` (Cypress) |
| Go, Rust | `*_test.go` (Go test); `*_test.rs`, `*_tests.rs` |
| Java, Kotlin, Scala, Groovy | Endung `Test`, `Tests`, `IT` oder `Spec` in der geschriebenen Groß-/Kleinschreibung (`Edit.java` ist kein `IT`-Test) |
| C#, F#, VB | Endung `Test` oder `Tests` |
| Ruby, PHP | `*_spec.rb` (RSpec), `*_test.rb`; `*Test.php` |
| Elixir, Erlang, Dart, Swift | `*_test.exs` (ExUnit), `*_SUITE.erl` (Common Test), `*_test.dart`, `*Tests.swift` |
| C, C++ | `*_test.c`/`.cc`/`.cpp`/`.cxx`, `*_unittest.cc`/`.cpp`, `test_*.c`/`.cpp` |
| Weitere | `*Spec.hs` (Hspec), `*.Tests.ps1` (Pester), `*_spec.lua` (busted), `test-*.R`/`test_*.R` (testthat), `runtests.jl`, `*.bats` (Bats), `*.robot` (Robot Framework), `*.feature` (Gherkin) |

Frameworks ergeben sich aus diesen Konventionen und aus Konfigurationsdateien
der Testwerkzeuge (`pytest.ini`, `tox.ini`, `noxfile.py`, `behave.ini`,
`.coveragerc`, `jest.config.*`, `vitest.config.*`, `.mocharc*`,
`ava.config.*`, `karma.conf.*`, `playwright.config.*`, `cypress.config.*`,
`wdio.conf.*`, `nightwatch.conf.*`, `stryker.conf.*`, `.nycrc*`, `.c8rc*`,
`nextest.toml`, `testng.xml`, `*.runsettings`, `xunit.runner.json`,
`phpunit.xml(.dist)`, `.rspec`, `dart_test.yaml`, `codecov.yml` …). Der Fund
nennt etwa „Testordnertypischer Name · 37 Testdateien · pytest, Playwright;
Inhalt nicht gelesen“, mit höchstens vier Frameworks und danach „+N“. Das
Projekt trägt die Summe aller Testdateien, deren nächstes Projekt es ist, als
Hinweis „Tests (…)“. Dateien in Testdaten, in Blättern (Umgebungen, Ausgaben,
Fremdcode) und in Ordnern, die ein Paketadapter beansprucht, zählen nicht.

Gelesen werden nur Namen. Ob Tests laufen, bestehen oder genutzt werden, bleibt
unbekannt. Testcode gehört zum Projekt: Testordner und Testdateien werden nie
ein Eintrag im Aufräumbericht und nie eine Regel des Kontextfilters; beides
entsteht nur für Testdaten und Test-Ausgaben.

### Liste

Unter **Projekte → Funde** lassen sich Funde nach Projekten,
Scratchpads/Skripten, Umgebungen/Builds, **Tests & Daten** (Testordner,
Testdaten, Test-Ausgaben und Datenbanken; Test-Ausgaben stehen auch unter
Umgebungen/Builds) und fehlendem Projektbezug filtern. Projekte (nach
Sicherheit) und Scratchpads stehen vor Notebooks, Skripten, Testdaten,
Datenbanken und Testordnern, danach folgen Umgebungen, Ausgaben und Caches.
Dieselbe Liste zeigt auch die bei der Projekterfassung erkannten Testordner,
Umgebungen, Build-Ausgaben und Caches. Die Ansicht ist auf 200 Funde begrenzt.
Die globale Suche fragt gespeicherte Fundmetadaten unabhängig von dieser Grenze
ab und öffnet auch Funde außerhalb der ersten Seite. Für große Bestände die
Suchwurzeln sinnvoll aufteilen. Teilläufe ersetzen den letzten vollständigen
Stand nicht.

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
höchstens 2 Mio. Einträge und 120 Sekunden. Geöffnet werden nur `.git/config`
und `.git/index` eines Repositorys im Projekt (siehe unten). Jeder Ordner wird
mit seinen Nachbardateien gelesen ([Regeln](../src/inventory/tidy.rs)):

| Klasse | Ordner oder Datei | Beleg |
|---|---|---|
| Wiederherstellbar | `node_modules` | npm-, Yarn-, pnpm- oder Bun-Lockfile daneben |
| Wiederherstellbar | `.venv`, `venv` | `uv.lock`, `poetry.lock`, `Pipfile.lock` oder `pdm.lock` daneben **und** `pyvenv.cfg` darin |
| Wiederherstellbar | `target` | `Cargo.toml` daneben und `CACHEDIR.TAG`, `.rustc_info.json`, `debug` oder `release` darin; oder `pom.xml` daneben und `maven-status`, `maven-archiver` oder `classes` darin |
| Wiederherstellbar | `bin`, `obj` | .NET-Projekt oder -Lösung daneben; `bin` zusätzlich mit `obj` daneben und `Debug` oder `Release` darin, `obj` mit `project.assets.json` darin (sonst bleibt `bin` unberührt) |
| Wiederherstellbar | `build` | Gradle-Datei daneben und `tmp`, `classes`, `libs` … darin; `CMakeLists.txt` daneben und `CMakeCache.txt` darin; `setup.py`/`pyproject.toml` daneben und `lib.*`, `bdist.*` oder `temp.*` darin (ein bloßes `lib` reicht nicht). Neben `package.json` nie: dort liegen oft selbst gepflegte Dateien (electron-builder, alte Vue-Vorlagen) |
| Wiederherstellbar | `dist`, `out`, `.next`, `.nuxt`, `.svelte-kit`, `.output` | Build-Datei daneben (`package.json`, `pyproject.toml`, Gradle, CMake …) |
| Wiederherstellbar | `.tox`, `.nox` | `tox.ini` oder `noxfile.py` daneben |
| Wiederherstellbar | `__pycache__`; `.pytest_cache`, `.mypy_cache`, `.ruff_cache` mit `CACHEDIR.TAG`; `.gradle`, `.turbo`, `.parcel-cache`, `.angular` mit der Spur ihres Werkzeugs; `.phpunit.cache` mit `test-results` oder `code-coverage` | Das Werkzeug legt sie beim nächsten Lauf neu an |
| Wiederherstellbar | `coverage` mit `lcov.info`, `coverage-final.json`, `clover.xml` …; `htmlcov` mit `status.json`; `test-results` mit `.last-run.json`; `test-output` mit `testng-results.xml`; `playwright-report` mit `data`; `.nyc_output` mit `processinfo`; `TestResults` mit `*.trx` oder `*.coverage`; `allure-results` mit `*-result.json` oder `*-container.json`; `blob-report` mit `report-*.zip` oder `report.zip`; `mutants.out` mit `outcomes.json` oder `mutants.json` | Der nächste Testlauf legt sie neu an |
| Wiederherstellbar | `lcov-report` mit Build-Datei daneben (`package.json`, `pyproject.toml` …) und `sorter.js`, `prettify.js` oder `block-navigation.js` darin; `surefire-reports` mit `pom.xml` daneben und `test-*.xml` darin, `failsafe-reports` mit `pom.xml` daneben und `failsafe-summary.xml` oder `test-*.xml` darin | Ohne die Datei daneben sind es oft Kopien oder veröffentlichte Berichte; Maven legt sie sonst in `target` an |
| Prüfen | Dieselben Ordner ohne ihren Beleg oder ohne die Spur darin; `.cache`; `build` neben `package.json` | Name allein kann auch Handarbeit sein (`coverage/` als Python-Paket) |
| Prüfen | `allure-report`, `test-reports`, `.stryker-tmp`, `.hypothesis` | Der Allure-Bericht hält den Verlauf früherer Läufe; Erzeuger von `test-reports` unbekannt; der Stryker-Arbeitsordner kann nach einem abgebrochenen Lauf Originaldateien enthalten; gefundene Hypothesis-Beispiele entstehen nicht sicher neu |
| Prüfen (Testdaten) | `testdata`, `test-data`, `test_data`, `testfiles`, `test-files`, `test-fixtures`, `test_fixtures`, `test-resources`, `test_resources`, `fixtures`, `__fixtures__`, `__snapshots__`, `cassettes` überall; `data`, `resources`, `samples`, `assets`, `fixtures`, `snapshots`, `golden`, `goldens` nur direkt in einem Testordner (`test`, `tests`, `spec`, `specs`, `__tests__`, `e2e`, `cypress`, `testing` … wie unter [Tests](#tests)) | Kann selbst gepflegt sein; eigene Summe, „Viele Testdaten“ ab 1 GB oder ab 100 MB bei mindestens der Hälfte des eigenen Projektinhalts |
| Prüfen | `logs`, Datenbankdateien, Logs ab 10 MB, jede Datei ab 100 MB | Zweck unbekannt |
| Prüfen (KI-Agent) | `.claude/worktrees` (Git-Arbeitskopien von Claude Code), `.playwright-mcp` (Screenshots, Mitschnitte, Konsolenprotokolle), `.aider.tags.cache.v3`/`.v4`, `.serena/cache`, `.specstory`, `.crush`, `.swarm`, `.hive-mind` | Nur am Namen erkannt, Herkunft also nicht belegt; eine Arbeitskopie kann unveröffentlichte Commits enthalten, ein Mitschnitt die einzige Spur eines Fehlschlags. Eigene Summe, nie wiederherstellbar |

**Git.** Liegt ein wiederherstellbarer Ordner in Git-Repositorys des Projekts
(`.git`-Ordner im Projekt oder darunter), liest der Bericht einmal je
Repository dessen Index (Version 2–4, SHA-1 und SHA-256, höchstens 128 MB; Git
wird nicht gestartet). **Jedes** Repository auf dem Weg muss bestätigen, dass es
keine Datei in dem Ordner führt – auch ein äußeres, das Dateien unter einem
inneren Repository eingecheckt hat. Führt ein Index eine Datei darin, wird der
Ordner zu „Prüfen · N Dateien darin sind in Git versioniert“ – etwa das
eingecheckte `dist/` einer GitHub Action. Sonst trägt der Beleg „nicht in Git“.
Lässt sich der Stand nicht lesen, bleibt der Ordner „Prüfen · Git-Stand
unbekannt“: ein beschädigter, geteilter (split) oder Sparse-Index, eine
unbekannte Pflichterweiterung, ein fehlender Index trotz Commits, Tags oder
abgekoppeltem HEAD, ein laufender Git-Befehl (`index.lock`), eine `.git`-Datei
(Worktrees, Submodule), deren Index woanders liegt, und ein Repository
**oberhalb** der freigegebenen Wurzel – etwa ein Paket, das einzeln aus einem
Monorepo registriert ist. Dort prüft mogumogu nur, ob es `.git` gibt, und liest
nichts darin; aufräumen lässt sich dann über das äußere Projekt. `.gitignore`
belegt nichts. Der Papierkorb-Plan prüft dasselbe direkt vor jedem Verschieben
erneut.

**Behalten.** Im Aufräumen-Dialog behält **„…“ behalten** einen der größten
wiederherstellbaren Ordner (auch per CLI `cleanup keep --project ID --path
web/dist`; wieder vorschlagen nur im Dashboard, denn das hebt einen Schutz auf). Ein behaltener Ordner, alles darin und jeder
Ordner, der ihn enthält, wird zu „Prüfen · von dir behalten“ und erscheint in
keinem Plan; ein schon bestätigter Plan stoppt vor dem Verschieben. Behalten
braucht die Lesefreigabe des Projekts: mogumogu öffnet den Weg dorthin ohne
Verknüpfungen und speichert den echten Ort, wie das Laufwerk ihn nennt (lange
Namen statt `LONGFO~1`). Darum gilt die Entscheidung für **jedes** Projekt, das
den Ordner erreicht – auch für eine zweite Registrierung in anderer
Schreibweise oder ein darin verschachteltes Projekt. Ein Ordner, der noch nicht
existiert, wird so behalten, wie er geschrieben ist, aber nie unter einem
Kurznamen. Die Entscheidung steht in der Datenbank (Schema v8, höchstens 500 je
Projekt und 5.000 insgesamt), nicht im Projekt. Sie nimmt nur heraus, sie macht
nie etwas wiederherstellbar; Groß- und Kleinschreibung werden Zeichen für
Zeichen wie unter Windows verglichen.

**Läufe und Docker.** Wie ein behaltener Ordner bleibt auch stehen, was ein
registrierter KI-Lauf gemeldet hat („gehört zu Lauf #3 claude-code
(fehlgeschlagen) (Arbeitsordner)“), und was ein Container eines freigegebenen
Docker-Kontexts einbindet („Container „web-1“ (exited) bindet ihn ein“), auch
über ein Volume, das einen Ordner bindet. Eine Haltung gilt unter der
Schreibweise, die sie nennt, und am echten Ort; jeder Ordner wird an seinem
echten Ort und in der registrierten Schreibweise verglichen, sodass Kurznamen,
Junctions und SUBST-Laufwerke nichts ausnehmen. Führt ein symbolischer Link
oder eine Junction auf dem Weg zu einer Freigabe oder einem nicht festen
Laufwerk, wird der echte Ort nicht bestimmt; die Haltung gilt dann in der
Schreibweise, die sie nennt. Den echten Ort bestimmt mogumogu je Entwurf oder
Ausführung einmal je Haltung. Ein Lauf hält seine Artefakte, bis du ihn nach
seinem Ende im Dashboard freigibst.
Einen Rest eines Agenten, den ein Lauf gemeldet hat, nennt der Bericht mit
diesem Lauf statt mit „nur am Namen erkannt“. Ist eine freigegebene Engine
nicht erreichbar, nennt sie nicht mehr die freigegebene Pipe oder antwortet
sie zu viel oder in falscher Form, steht das als Hinweis am Projekt, nichts im
Projekt ist wiederherstellbar, und jeder Papierkorb-Plan wird blockiert.

**Überwachte Datenbanken.** Eine registrierte Datenbankdatei oder ein
Datenordner steht als „Prüfen · überwachte Datenbank „…““ im Bericht, mit
jedem Ordner, der sie enthält, und erscheint in keinem Plan. Die Registrierung
speichert den Ort wie geschrieben und den echten Ort, gebunden an die Identität
des Ordners (bei einer Datei die ihres Ordners); Verknüpfungen, Freigaben und
Wechsellaufwerke werden abgelehnt. Erkannt wird die Engine an Namen: an der
Dateiendung (`.sqlite`, `.db`, `.duckdb` …), an Dateien im Datenordner
(`PG_VERSION`, `ibdata1`, `WiredTiger`, `dump.rdb`) oder am Namen eines
Volumes (`pgdata`, `mysql`, `mongo` …). Ein Datenordner namens `data` oder
`db` heißt nach Engine und übergeordnetem Ordner, etwa „PostgreSQL 16“. Jeder
vollständige Bericht misst überwachte Dateien und Ordner und speichert die
Messung (je Datenbank die letzten 100); das Wachstum vergleicht die älteste und
neueste Messung der letzten 90 Tage. Eine Messung, die nicht gelingt, steht mit
Grund im Verlauf, ohne Größe.

**Verlauf.** Jeder gemessene Bericht wird je Projekt gespeichert: Größen und
die 20 größten Einträge, die neuesten 30 je Projekt. Das Wachstum vergleicht
den ältesten und den neuesten vollständigen Bericht der letzten 90 Tage und
nennt die drei am stärksten wachsenden Einträge. Ein Eintrag, der früher nicht
unter den größten war, zählt dabei ab null.

**Belegter Platz.** Neben der logischen Größe summiert der Bericht den Platz,
den das Laufwerk für wiederherstellbare Ordner reserviert hat (auf Cluster
gerundet, kleiner bei Kompression). Eine Datei ab 64 KiB, die das Projekt über
mehrere Hardlinks erreicht, zählt dabei einmal (erkannt an der Datei-ID);
kleinere zählen je Name, und ein Hardlink in ein anderes Projekt zählt in
beiden. Die logische Größe zählt weiter je Name.

Alles andere zählt zum Projekt, auch Testordner und Testdateien; `.git`, `.hg`
und `.svn` erscheinen getrennt als Versionsverwaltung. Als letzte Änderung gilt
die neueste Datei außerhalb von Ausgaben, Caches und Versionsverwaltung. Ein
Projekt, das seit über 180 Tagen unverändert ist und Wiederherstellbares
enthält, trägt „Ruht“. Name und Alter allein machen nichts wiederherstellbar.

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
Besitzerprozesses, bis er neu erstellt wird oder mogumogu endet; gespeichert
werden die gemessene Projektgröße und der Verlauf (siehe oben). Er entfernt nichts. Eine Freigabe
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

Den früheren Ordnerbrowser gibt es seit dem 24. Oktober 2026 nicht mehr;
Ordnergrößen zeigt die Speicheranalyse (unten) unter eigener Freigabe. Was ein
Projekt wiederherstellbar belegt, zeigt weiterhin der Aufräumbericht.

## Speicheranalyse

**Speicher → Speicheranalyse** zeigt, was in einem Ordner oder auf einer ganzen
lokalen Festplatte Platz belegt: Ordner mit Größe, belegtem Platz und Anteil am
angezeigten Ordner, die größten Dateien, Dateitypen nach Endung,
Duplikat-Kandidaten und eine Flächenkarte der 40 größten Einträge (der Rest als
eine Fläche). Gelesen werden nur Verzeichniseinträge: Name, logische Größe,
belegter Platz auf dem Datenträger und Änderungsdatum. Kein Dateiinhalt, keine
Prüfsumme, kein Programmstart.

**Freigabe.** Eine eigene Art „Speicheranalyse (nur Dateimetadaten)“ (Schema
v7), gebunden an die Identität des Ordners oder Laufwerks und nur auf lokalen
Festplatten. Jeder Pfadbestandteil wird handle-relativ geöffnet; eine
Verknüpfung unterwegs lehnt die Freigabe ab. Die Lesefreigabe eines Projekts
genügt für dieses Projekt; Namenssuche, KI- und Cache-Freigaben genügen nicht.
Erteilt wird sie über **Speicher analysieren** auf einer Laufwerkskarte,
**Ordner freigeben** im Reiter oder `storage approve --path`. Sie ist kein
Inventarbereich: **Jetzt erfassen** übergeht sie.

**Ablauf.** Analysiert wird nur auf Wunsch, höchstens 4 Mio. Einträge, 64
Ebenen und drei Minuten; **Abbrechen** in der Erfassungsleiste beendet den
Durchgang. Ein Eintrag belegt 40 Bytes plus seinen Namen; gemessen hielt die
Analyse der Systemfestplatte dieses Rechners (3,1 Mio. Einträge) 198 MiB, ein
Datenlaufwerk mit 1,4 Mio. Einträgen 83 MiB ([Messbericht](MESSBERICHT.md)).
Eine Seite ordnet nur die Einträge bis zu sich selbst, nicht die ganze Liste. Das Ergebnis liegt nur im Arbeitsspeicher des Besitzerprozesses:
höchstens eine Analyse, ersetzt durch die nächste und verworfen beim Schließen
des Fensters, beim Widerruf, mit `storage clear` oder, solange kein Fenster
offen ist, nach zehn Minuten ohne Anfrage. Namen gelangen nicht in die
Datenbank. Ordnernummern gelten nur innerhalb ihrer Analyse; mit dem Token aus
dem Feld `analysis` lehnt die CLI eine Seite ab, wenn inzwischen neu analysiert
wurde. Blättern (Seiten zu 60, höchstens 200 Einträge), Sortieren (Größe, auf
Datenträger, Name, Änderung) und Filtern (Teilwort, oder `*` und `?` für den
ganzen Namen; ein zu aufwendiges Muster wird abgelehnt) lesen das Dateisystem
nicht erneut. Ein nicht erreichbarer Ordner wird beim nächsten Analysieren erneut versucht; ein
ersetzter Ordner braucht eine neue Freigabe. Den Status einer
Projektfreigabe ändert die Analyse nur, wenn der Ordner ersetzt wurde; ob ein
Projekt erreichbar ist, hält die Erfassung fest. **Größe messen** gilt nicht für
Speicheranalyse-Freigaben.

**Grenzen.** Verknüpfungen, Junctions und Cloud-Platzhalter werden ausgelassen;
sie machen das Ergebnis ebenso unvollständig wie ein Limit, ein unlesbarer Ordner
oder ein Abbruch. Dann steht vor der Summe „Mindestens“, und die Gesamtgröße
bleibt unbekannt; als Grund steht ein Limit oder Abbruch vor einer
ausgelassenen Verknüpfung. Ein Ordner, unter dem etwas nicht gelesen wurde,
zeigt „mind.“ vor der Größe und „≥“ vor dem Anteil, ein gar nicht gelesener
„nicht gelesen“ statt 0 B; die CSV-Spalte `complete` sagt dasselbe je Zeile.
Eine Verknüpfung macht ihren Ordner nicht unvollständig, ihr Ziel gehört nicht
zu ihm. Erst ein Laufwerk mit mehr als 4 Mio. Einträgen bleibt am Limit
unvollständig; Systemordner ohne Leserecht bleiben es immer. Hardlinks zählen je Namen; Verwaltungsdaten des Dateisystems (MFT,
Ordnerindizes) fehlen. Duplikat-Kandidaten haben nur denselben Namen und dieselbe
Größe; Inhalte werden nicht verglichen. Pfade erscheinen redigiert; ein langer
Pfad wird vorn gekürzt, der Dateiname bleibt. **CSV
kopieren …** zeigt eine Tabellenseite zum Kopieren an (`storage export` gibt sie
auf stdout aus); Namen, die eine Tabellenkalkulation als Formel lesen würde,
werden entschärft, und nichts wird in den analysierten Ordner geschrieben.
Löschen, Verschieben und Öffnen im Explorer gibt es hier nicht. Eine Größe
belegt keine Entbehrlichkeit.

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
mogumogu-cli storage approve --path D:\                 # eigene Freigabe, nur Metadaten
mogumogu-cli storage analyze --scope 9
mogumogu-cli storage tree --scope 9 --folder 2 --sort allocated
mogumogu-cli storage files --scope 9 --search "*.zip"
mogumogu-cli storage export --scope 9 --limit 200 > seite.csv
mogumogu-cli ai content --scope 1 --relative .agents/skills/beispiel/SKILL.md
```

## Prüfnachweise und Grenzen

`tests/package_scopes.rs` prüft Erkennung, Kontext, Zuordnung, Schutz,
Idempotenz, Teilläufe, die Reihenfolge samt Testordnern und den Zugriff
außerhalb des Seitenlimits. Testkonventionen, Blattregel und Aufräumregeln
prüfen die Windows-Tests in `src/inventory/discovery.rs` und
`src/inventory/tidy.rs`.
`tests/disk.rs` prüft Freigaben, Austausch, Widerruf, Junctions, Limits,
Summierung und Demotrennung. `tests/ai_content.rs` prüft flüchtige Inhalte,
Redaktion, Austausch, Widerruf, Pfadausbruch und Format-/Größenlimits.
Die Leseerweiterungen wurden zusätzlich von einem getrennten Agenten geprüft.
Dies ersetzt keine Windows-11- oder menschliche Sicherheitsabnahme.

Quellen: [Windows-Laufwerke](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getlogicaldrives),
[Laufwerkstyp](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew),
[Kapazität](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdiskfreespaceexw),
[Slint TextEdit](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/textedit/).
