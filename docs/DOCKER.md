# Docker für Entwicklung, Prüfungen und den Windows-Build

Das Linux-Image prüft den portablen Rust-Kern und baut auf Wunsch die
Windows-Programme, ohne dass auf dem Rechner Rust, Visual Studio oder ein
Windows SDK installiert sein muss. Tray, Dashboard, Named Pipes, Papierkorb und
die identitätsgebundene Windows-Lesegrenze laufen und werden geprüft nur nativ
auf Windows 10/11. Docker stellt keinen Zugriff auf das Inventar des
Hostrechners bereit.

## Windows-Programme im Container bauen

Voraussetzung ist nur Docker Desktop mit Linux-Containern. Am einfachsten
`rebuild.cmd` doppelklicken: Ist kein Rust installiert, baut es im Container,
beendet vorher eine laufende Instanz aus `dist` geordnet, startet sie danach
wieder und zeigt den Pfad der EXE. Mit Rust erzwingt `.\rebuild.ps1 -Docker`
den Container; `-Test` führt dort zuerst die Kernprüfungen (`validated`) aus.
Ohne Skript:

```powershell
docker build --target windows-exe --output dist/mogumogu .
```

Die Stufe `windows` installiert mingw-w64 und das Rust-Ziel
`x86_64-pc-windows-gnu` und kompiliert `mogumogu.exe` und `mogumogu-cli.exe` im
Release-Profil, Icon eingeschlossen (`windres`). `windows-exe` enthält nur die
fertigen Dateien samt Lizenztexten, `Cargo.lock` und `FONT-LICENSE.txt`, also
denselben Inhalt wie `build.ps1 -NoRun`. Der erste Build lädt die Crates und
dauert einige Minuten; danach liegen Downloads und Kompilat im BuildKit-Cache
(`mogumogu-cargo`, `mogumogu-target-windows`).

Unterschiede zum nativen Build: GNU- statt MSVC-Toolchain, SQLite mit gcc statt
MSVC kompiliert, die Windows-spezifischen Tests (Lesegrenze, Pipe, Papierkorb)
laufen nicht. Die EXE lädt nur Windows-Systembibliotheken (keine mingw-DLLs);
am 24. Oktober 2026 liefen damit Besitzer, CLI, Erfassung, Aufräumbericht,
Kontextfilter und das Dashboard auf Windows 10. Für Abnahmen und Releases
bleibt der native MSVC-Build (`build.ps1`, `scripts/release.ps1`) maßgeblich.

## Lokal ausführen

Voraussetzung: Docker Engine mit Compose v2 und Linux-Containern; auf Windows
Docker Desktop starten und die Linux-Engine verwenden. Im Repository:

```powershell
docker compose run --build --rm checks
```

Der Befehl kopiert den aktuellen Quellstand ins Image und führt
`scripts/check.py --core` aus: statische Verträge, Formatierung, Clippy mit
`-D warnings` sowie die Tests des Kerns mit lokalen Fixtures. Die Anwendung
selbst stellt keine Netzwerkverbindungen her.

Cargo-Downloads und Compiler-Ausgaben bleiben in den benannten Volumes
`cargo-cache` und `target-cache`. Nach dem ersten erfolgreichen Lauf lässt
sich derselbe Stand ohne neue Dependency-Downloads prüfen:

```powershell
docker compose run --rm checks --core --offline
```

Nach Quelländerungen erneut `--build` verwenden. Ein neues Lockfile benötigt
gegebenenfalls zuerst einen Lauf ohne `--offline`. Die Volumes enthalten
ausschließlich Build-Caches; es gibt keine Bind-Mounts, Ports, Dienste oder
Host-Inventardaten. Der Prozess läuft mit UID/GID 10001, schreibgeschütztem
Image und einem temporären `/tmp`.

## Prüfung während des Image-Builds

```powershell
docker build --target validated --progress plain -t mogumogu-checks .
```

`checks` ist das ausführbare Entwicklungsimage; `validated` führt dieselben
Prüfungen zusätzlich während des Builds aus und ist als letzte Stufe das
Standardziel des Dockerfiles. BuildKit speichert Compiler- und Registry-Caches außerhalb des
Images. Ein gecachter erfolgreicher Build gilt für denselben Quellstand;
Compose führt die Prüfungen bei jedem Aufruf erneut aus.

Die Rust-Version entspricht `rust-toolchain.toml`; das Basisimage ist zusätzlich
per Digest festgelegt. Bei einem Toolchainwechsel Dockerfile, Toolchain-Datei
und Digest gemeinsam aktualisieren. `scripts/check.py` verweigert eine
abweichende Compiler-Version. `.dockerignore` erlaubt ausschließlich die
benötigten Repository-Eingaben, einschließlich `rustfmt.toml` und der
kanonischen sowie synchronisierten Skills. Persönliche Einstellungen,
Secrets, `.git`, lokale Datenbanken, Exporte und Host-Builds sind ausgeschlossen.

## Native Prüfungen

```powershell
python scripts/check.py
.\build.ps1 -NoRun
```

Auf Windows prüft der gemeinsame Runner zusätzlich das Desktopfeature und
die UI-Binärtests. Windows-Cleanup-Fixtures liegen unter
`tests/core/windows_cleanup.rs` und laufen ausschließlich nativ; portable
Speicher-, Migrations-, Generationen- und Review-Verträge laufen auch im
Container. Die beiden CI-Workflows ergänzen sich. Manuelle Prüfungen aus
[WINDOWS-ABNAHME.md](WINDOWS-ABNAHME.md) und Gate G5 bleiben erforderlich.

Grundlagen: [Docker-Build-Praktiken](https://docs.docker.com/build/building/best-practices/),
[Build-Caches](https://docs.docker.com/build/cache/optimize/).
