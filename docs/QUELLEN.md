# Technische Grundlagen

Offizielle Dokumentation, die der Umsetzung zugrunde liegt (Stand Oktober 2026). Links belegen API- und Werkzeuggrundlagen, keine Performancewerte; die tatsächlich gebauten Versionen stehen in `Cargo.lock` und im [Validierungsprotokoll](VALIDIERUNG.md).

## Plattform und Laufzeit
- Slint 1.18.1 Rust-API: https://docs.rs/slint/1.18.1/slint/
- Slint SystemTrayIcon: https://docs.slint.dev/latest/docs/slint/reference/window/systemtrayicon/
- Slint-Buildintegration (`EmbedResourcesKind`): https://docs.rs/slint-build/1.18.1/slint_build/
- rusqlite 0.40 (inkl. Backup-API): https://docs.rs/rusqlite/0.40.2/rusqlite/
- SQLite WAL und `PRAGMA synchronous`: https://sqlite.org/wal.html · https://sqlite.org/pragma.html#pragma_synchronous
- SQLite Online-Backup-API: https://sqlite.org/backup.html
- Rust `File::try_lock`: https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock

## Windows-APIs der Sicherheitsgrenzen
- Besitzerstart ohne Handle-Vererbung (`CreateProcessW`, expliziter Programmpfad, beschreibbare UTF-16-Kommandozeile): https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw
- `NtCreateFile` (handle-relatives Öffnen über `OBJECT_ATTRIBUTES.RootDirectory`, `FILE_OPEN_REPARSE_POINT`): https://learn.microsoft.com/windows/win32/api/winternl/nf-winternl-ntcreatefile
- `GetFileInformationByHandleEx` (`FileIdInfo`, `FileIdExtdDirectoryInfo`): https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex
- `FILE_DISPOSITION_INFO_EX` (POSIX-Löschsemantik am Handle): https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_disposition_info_ex
- Reparse Points und Cloud-Platzhalter (Dateiattribute): https://learn.microsoft.com/windows/win32/fileio/file-attribute-constants
- Job-Objekte (verschachtelt, `JobObjectBasicAccountingInformation`): https://learn.microsoft.com/windows/win32/procthread/job-objects
- Named Pipes: Sicherheit, `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE`: https://learn.microsoft.com/windows/win32/ipc/named-pipe-security-and-access-rights
- `GetNamedPipeClientProcessId` / `GetNamedPipeServerProcessId`: https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-getnamedpipeclientprocessid
- `ReadDirectoryChangesW` (Pufferüberlauf ⇒ Abgleich, über die Crate `notify`): https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-readdirectorychangesw
- Uninstall-Registrierung installierter Programme: https://learn.microsoft.com/windows/win32/msi/uninstall-registry-key

## Formate
- npm package-lock: https://docs.npmjs.com/cli/v11/configuring-npm/package-lock-json
- pnpm-Lockfile und virtueller Store: https://pnpm.io/symlinked-node-modules-structure
- Python: Installierte Distributionen (`*.dist-info`): https://packaging.python.org/en/latest/specifications/recording-installed-packages/
- Python: `.pth`-Dateien werden beim Start ausgeführt: https://docs.python.org/3/library/site.html
- uv-Lockfile: https://docs.astral.sh/uv/concepts/projects/layout/#the-lockfile
- Cargo.lock und Quellen: https://doc.rust-lang.org/cargo/reference/resolver.html
- NuGet-Lockfile und zentrale Paketverwaltung: https://learn.microsoft.com/nuget/consume-packages/central-package-management
- Agent-Skills-Format: https://agentskills.io/specification
- Die Quellen je KI-Profil stehen im Katalog `profiles/ai-catalog.toml` (Feld `source`).

## Lizenzen
- Slint-Lizenzoptionen: https://slint.dev/pricing
- Windows-10-Lebenszyklus: https://learn.microsoft.com/lifecycle/products/windows-10-home-and-pro

## Paketbelege (Ergänzung 17. Oktober 2026)

- npm Registry-Metadaten und Abkündigung: https://github.com/npm/registry/blob/main/docs/responses/package-metadata.md · https://docs.npmjs.com/deprecating-and-undeprecating-packages-or-package-versions/
- Python-Projektstatus / Simple JSON: https://packaging.python.org/en/latest/specifications/project-status-markers/ · https://docs.pypi.org/api/index-api/
- PyPI Release-Metadaten/Yanks: https://docs.pypi.org/api/json/ · https://docs.pypi.org/project-management/yanking/
- Cargo-Yanks und Installationsdatensätze: https://doc.rust-lang.org/cargo/commands/cargo-yank.html · https://doc.rust-lang.org/cargo/guide/cargo-home.html
- NuGet-Metadaten/Abkündigung und Versionsnormalisierung: https://learn.microsoft.com/en-us/nuget/api/registration-base-url-resource · https://learn.microsoft.com/en-us/nuget/concepts/package-versioning
- uv-Datenorte: https://docs.astral.sh/uv/reference/storage/ · https://docs.astral.sh/uv/configuration/environment/
- Einfache GitHub-Workflow-Referenzen: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax
- Konkrete Versionsordnung: https://docs.rs/pep440_rs/0.7.3/pep440_rs/struct.Version.html · https://docs.rs/semver/1.0.28/semver/struct.Version.html

Dokumentationsseiten werden von der Anwendung nicht abgerufen. Öffentliche
Registry-Metadaten werden nur bei separat freigegebenen Quellenhinweisen abgefragt.
