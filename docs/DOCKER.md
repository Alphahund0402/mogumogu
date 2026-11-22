# Docker für Entwicklung, Prüfungen und den Windows-Build

Das Linux-Image prüft den portablen Rust-Kern und baut auf Wunsch die
Windows-Programme, ohne dass auf dem Rechner Rust, Visual Studio oder ein
Windows SDK installiert sein muss. Tray, Dashboard, Named Pipes, Papierkorb und
die identitätsgebundene Windows-Lesegrenze laufen und werden geprüft nur nativ
auf Windows 10/11. Docker stellt keinen Zugriff auf das Inventar des
Hostrechners bereit.

Docker Desktop braucht eine eigene virtuelle Maschine und mehrere GB
Arbeitsspeicher. Auf schwächeren Rechnern lädt `.\rebuild.ps1 -Download` statt
eines Builds den geprüften Build aus der CI oder das neueste Release; siehe
[Entwicklung](ENTWICKLUNG.md#auf-schwächeren-rechnern-entwickeln).

## Windows-Programme im Container bauen

Voraussetzung ist nur Docker Desktop mit Linux-Containern. Am einfachsten
`rebuild.cmd` doppelklicken: Ist kein Rust installiert, baut es im Container
(läuft Docker nicht, lädt es stattdessen den fertigen Build),
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

## Isolierte Test- und Benchmark-Sandbox (offline)

Für reproduzierbare Tests und Kern-Benchmarks ohne Netzwerk und ohne Host-Volumes
bietet [compose.yaml](../compose.yaml) die Stufe `test-env` (gesteuert über
[docker_sandbox.py](../scripts/docker_sandbox.py)):

```powershell
# Alle statischen Verträge, Kern-Tests und 3× Release-Benchmark (offline, 4 CPUs, 2 GiB RAM)
docker compose run --build --rm test-sandbox

# Nur den Release-Benchmark ausführen (z. B. 5 Läufe)
docker compose run --rm test-sandbox --bench --runs 5

# Gleiche Offline-Sandbox unter engen Ressourcen (1 vCPU, 512 MiB RAM)
docker compose run --build --rm test-weak-hw --bench
```

Beim Build kompiliert `test-env` Formatierung, Clippy, die portablen Testprogramme
und den Release-Benchmark (`tests/performance.rs::portable_core_benchmark`) in
`/opt/mogumogu-test` und bündelt den per SHA-256 geprüften offiziellen Linux-`osv-scanner`
v2.7.1 (geladen über `mogumogu-advisories fetch-scanner`). Zur Laufzeit startet
der Container vollständig isoliert: `network_mode: none` (nur Loopback `lo`),
schreibgeschütztes Root-Dateisystem, keine Volumes, `cap_drop: [ALL]`,
`no-new-privileges`, Benutzer `10001:10001`, flüchtiges `tmpfs` unter `/tmp` sowie
feste CPU-/RAM-/PID-Grenzen. Dadurch laufen alle Offline-Watcher- und Audit-Tests
mit synthetischen Advisory-ZIPs vollständig gegen den echten OSV-Scanner (ohne
übersprungene Tests). Windows-spezifische Tests (Tray, Named Pipe, `NtCreateFile`,
Papierkorb) bleiben davon unberührt und laufen weiterhin nativ auf Windows 10/11.

## Sicherheits-Audit für `Cargo.lock` (offline)

Die Stufe `security-audit` prüft mogumogus eigene `Cargo.lock` gegen das öffentliche
OSV-/RustSec-Archiv (`crates.io/all.zip`) und die befristete Triage-Liste
[security/advisory-triage.toml](../security/advisory-triage.toml):

```powershell
# Mit frischem crates.io-Feed bauen und komplett offline auswerten
docker compose run --build --rm security-audit

# Mit explizitem Tages-Cache-Bust bauen (wie in der wöchentlichen CI)
docker build --target security-audit --build-arg OSV_FEED_DATE=2026-11-22 -t mogumogu-security-audit .
docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges:true --tmpfs /tmp:rw,nosuid,nodev,size=256m,mode=1777 mogumogu-security-audit
```

Nur der Image-Build lädt den verifizierten `osv-scanner` v2.7.1 und das öffentliche
`crates.io`-Archiv; zur Laufzeit arbeitet `security-audit` mit `network_mode: none`,
schreibgeschütztem Root-Dateisystem und ohne Host-Volumes. Ein neues Advisory oder
eine abgelaufene Triage-Frist bricht mit Exit-Code `1` ab, ein fehlender oder über
sieben Tage alter Feed mit Exit-Code `2`. Der Workflow
[security-audit.yml](../.github/workflows/security-audit.yml) führt diese Prüfung
bei Änderungen an `Cargo.lock` und wöchentlich aus.

Für andere Projekte und CI/CD-Pipelines bietet [packages/mogumogu-advisories/](../packages/mogumogu-advisories/README.md)
ein eigenständiges Image samt [compose.yaml](../packages/mogumogu-advisories/compose.yaml)
(`docker compose -f packages/mogumogu-advisories/compose.yaml run --rm refresh` und
`... run --rm audit`), mit vorinstalliertem `mogumogu-advisories`, verifiziertem
`osv-scanner` v2.7.1, automatischer Triage-Erkennung sowie SARIF 2.1.0, HTML und JSON.

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
per Digest festgelegt. Es ist die schlanke Variante (`rust:<Version>-slim-bookworm`):
gcc und die C-Header reichen für das gebündelte SQLite, OpenSSL und pkg-config
braucht kein Crate. Python, rustfmt und Clippy installiert nur die Stufe
`checks`, mingw-w64 nur die Stufe `windows`. Bei einem Toolchainwechsel Dockerfile, Toolchain-Datei
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
