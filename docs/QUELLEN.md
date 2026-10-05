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

Die Anwendung enthält keine automatische Verbindung zu diesen Seiten.
