# Docker für Entwicklung und Prüfungen

Das Linux-Image prüft den portablen Rust-Kern. Tray, Dashboard, Named Pipes,
Job Objects und die identitätsgebundene Windows-Lesegrenze werden weiterhin
nativ auf Windows 10/11 gebaut und geprüft. Docker stellt keinen Zugriff auf
das Inventar des Hostrechners bereit.

## Lokal ausführen

Voraussetzung: Docker Engine mit Compose v2 und Linux-Containern; auf Windows
Docker Desktop starten und die Linux-Engine verwenden. Im Repository:

```powershell
docker compose run --build --rm checks
```

Der Befehl kopiert den aktuellen Quellstand ins Image und führt
`scripts/check.py --core` aus: statische Verträge, Formatierung, Clippy mit
`-D warnings` sowie Tests mit und ohne das optionale Netzwerkfeature.
Netzwerkfreigaben der Anwendung werden dadurch nicht eingeschaltet; Tests
verwenden lokale Fixtures und simulierte Registries.

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
Prüfungen zusätzlich während des Builds aus und ist das Standardziel des
Dockerfiles. BuildKit speichert Compiler- und Registry-Caches außerhalb des
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
die UI-Binärtests. Windows-Session- und Cleanup-Fixtures liegen unter
`tests/core/windows_cleanup.rs` und laufen ausschließlich nativ; portable
Speicher-, Migrations-, Generationen- und Review-Verträge laufen auch im
Container. Die beiden CI-Workflows ergänzen sich. Manuelle Prüfungen aus
[WINDOWS-ABNAHME.md](WINDOWS-ABNAHME.md) und Gate G5 bleiben erforderlich.

Grundlagen: [Docker-Build-Praktiken](https://docs.docker.com/build/building/best-practices/),
[Build-Caches](https://docs.docker.com/build/cache/optimize/).
