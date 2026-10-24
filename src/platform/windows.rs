//! Windows implementation. Every function keeps raw handles private and
//! documents why each `unsafe` call is sound.
//!
//! Safety model for filesystem access (Implementierungsplan F-06):
//! - Roots are opened by path *once*, without following a final reparse
//!   point, and their identity is compared with the approved identity.
//! - Everything below a root is opened *relative to the parent handle* via
//!   `NtCreateFile` with `FILE_OPEN_REPARSE_POINT`, one component at a time.
//!   Replacing an intermediate directory by a junction therefore cannot
//!   redirect a later open: there is no path re-resolution.
//! - Reparse points and cloud placeholders are rejected after opening, on
//!   the handle itself.
//! - Cleanup only moves a folder to the recycle bin. The caller holds the
//!   parent open without delete sharing (so it cannot be renamed) and
//!   Windows refuses to rename a folder with open handles below it, so no
//!   folder on the way can be swapped; the folder's identity was verified on
//!   that parent handle right before.
use super::{DirEntryInfo, FileIdentity, HandleStat, attributes, filetime_to_unix};
use crate::domain::DriveUsage;
use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::io::{self, Read, Write};
use std::marker::PhantomData;
use std::mem::{offset_of, size_of, zeroed};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr;
use std::time::{Duration, Instant};

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_FOR_BACKUP_INTENT, FILE_OPEN_REPARSE_POINT,
    FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
};
use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, ERROR_NO_MORE_FILES,
    ERROR_NOT_SUPPORTED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL,
    INVALID_HANDLE_VALUE, LocalFree, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_BASIC_INFO, FILE_DISPOSITION_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
    FILE_ID_EXTD_DIR_INFO, FILE_ID_INFO, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileBasicInfo, FileDispositionInfo, FileIdExtdDirectoryInfo,
    FileIdExtdDirectoryRestartInfo, FileIdInfo, FileStandardInfo, FlushFileBuffers, GetDiskFreeSpaceExW, GetDriveTypeW,
    GetFileInformationByHandle, GetFileInformationByHandleEx, GetLogicalDrives, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
    ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, SYNCHRONIZE, SetFileInformationByHandle, WriteFile,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW, RegOpenKeyExW,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

const TIME_ZONE_ID_STANDARD: u32 = 1;
const TIME_ZONE_ID_DAYLIGHT: u32 = 2;
const FILE_TRAVERSE: u32 = 0x20;

// ---------------------------------------------------------------- helpers

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

/// Drive-letter metadata only. Directory contents are never opened here.
/// Remote, optical and unknown drives are listed without probing capacity;
/// removable media are also left untouched to avoid waking absent devices.
pub fn logical_drives() -> io::Result<Vec<DriveUsage>> {
    // SAFETY: GetLogicalDrives has no pointer arguments or output buffers.
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut drives = Vec::new();
    for index in 0..26_u32 {
        if mask & (1 << index) == 0 {
            continue;
        }
        let root = format!("{}:\\", char::from(b'A' + index as u8));
        let path = wide(OsStr::new(&root));
        // SAFETY: path is a live NUL-terminated drive root, with trailing slash.
        let drive_type = unsafe { GetDriveTypeW(path.as_ptr()) };
        let (kind, mut status) = match drive_type {
            2 => ("Wechseldatenträger", "Kapazität nicht abgefragt"),
            3 => ("Lokales Laufwerk", "Vom Betriebssystem gemeldet"),
            4 => ("Netzlaufwerk", "Kapazität nicht abgefragt"),
            5 => ("Optisches Laufwerk", "Kapazität nicht abgefragt"),
            6 => ("RAM-Laufwerk", "Vom Betriebssystem gemeldet"),
            _ => ("Unbekannter Typ", "Nicht erreichbar oder nicht unterstützt"),
        };
        let mut total = 0_u64;
        let mut free = 0_u64;
        let mut known = false;
        if matches!(drive_type, 3 | 6) {
            // SAFETY: path stays alive; total and free are writable u64 values.
            // Only generated local drive roots reach the API, never a config path.
            known = unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, &mut total, ptr::null_mut()) } != 0;
            if !known {
                status = "Kapazität derzeit nicht verfügbar";
            }
        }
        drives.push(DriveUsage {
            label: root.clone(),
            root,
            kind: kind.into(),
            total_bytes: known.then_some(total),
            free_bytes: known.then_some(free.min(total)),
            used_bytes: known.then_some(total.saturating_sub(free)),
            project_bytes: None,
            status: status.into(),
            searchable: matches!(drive_type, 3 | 6),
            search: None,
        });
    }
    Ok(drives)
}

/// Long-path form for an already validated absolute drive path.
fn wide_long_path(path: &Path) -> Vec<u16> {
    let text = path.as_os_str();
    let mut prefixed = OsString::from(r"\\?\");
    prefixed.push(text);
    if text.to_string_lossy().starts_with(r"\\") { wide(text) } else { wide(&prefixed) }
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the handle was just returned by a successful Win32 call and is
    // owned exclusively by the returned wrapper, which closes it once.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle()
}

fn not_safe(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message.to_string())
}

/// Reads a fixed-size `FILE_INFO_BY_HANDLE_CLASS` record.
fn handle_info<T: Default>(handle: HANDLE, class: i32) -> io::Result<T> {
    let mut value = T::default();
    // SAFETY: `value` is a properly aligned, writable `T` and the size passed
    // is exactly `size_of::<T>()`; the class matches `T` at every call site.
    let ok = unsafe {
        GetFileInformationByHandleEx(handle, class, (&raw mut value).cast::<c_void>(), size_of::<T>() as u32)
    };
    if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(value) }
}

fn identity_of(handle: HANDLE) -> io::Result<FileIdentity> {
    if let Ok(info) = handle_info::<FILE_ID_INFO>(handle, FileIdInfo) {
        return Ok(FileIdentity { volume: info.VolumeSerialNumber, id: info.FileId.Identifier });
    }
    // FAT/exFAT do not support FileIdInfo; use the classic 64-bit index.
    // SAFETY: zeroed is a valid bit pattern for this plain C struct.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    // SAFETY: `info` is writable and correctly sized for this call.
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    let mut id = [0_u8; 16];
    id[..8].copy_from_slice(&index.to_le_bytes());
    Ok(FileIdentity { volume: u64::from(info.dwVolumeSerialNumber), id })
}

fn stat_of(handle: HANDLE) -> io::Result<HandleStat> {
    let basic: FILE_BASIC_INFO = handle_info(handle, FileBasicInfo)?;
    let standard: FILE_STANDARD_INFO = handle_info(handle, FileStandardInfo)?;
    Ok(HandleStat {
        identity: identity_of(handle)?,
        attributes: basic.FileAttributes,
        size: standard.EndOfFile.max(0) as u64,
        modified: filetime_to_unix(basic.LastWriteTime),
    })
}

/// Validates a single path component for handle-relative opening.
fn component(name: &OsStr) -> io::Result<Vec<u16>> {
    let units: Vec<u16> = name.encode_wide().collect();
    let invalid = units.is_empty()
        || units.len() > 255
        || units.iter().any(|&u| u == u16::from(b'\\') || u == u16::from(b'/') || u == 0)
        || units == [u16::from(b'.')]
        || units == [u16::from(b'.'), u16::from(b'.')];
    if invalid {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "ungültige Pfadkomponente"));
    }
    Ok(units)
}

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// Opens `name` relative to `parent` without any path re-resolution.
fn open_relative(parent: HANDLE, name: &OsStr, access: u32, options: u32, share: u32) -> io::Result<OwnedHandle> {
    create_relative(parent, name, access, options, share, FILE_OPEN)
}

/// Like [`open_relative`] with an explicit create disposition.
fn create_relative(
    parent: HANDLE,
    name: &OsStr,
    access: u32,
    options: u32,
    share: u32,
    disposition: u32,
) -> io::Result<OwnedHandle> {
    let mut units = component(name)?;
    let bytes = (units.len() * 2) as u16;
    let object_name = UNICODE_STRING { Length: bytes, MaximumLength: bytes, Buffer: units.as_mut_ptr() };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent,
        ObjectName: &object_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    // SAFETY: zeroed is a valid initial state for the status block.
    let mut status_block: IO_STATUS_BLOCK = unsafe { zeroed() };
    let mut handle: HANDLE = ptr::null_mut();
    // SAFETY: all pointers refer to live stack values for the duration of the
    // synchronous call; `units` outlives `object_name`; `parent` is a valid
    // directory handle owned by the caller.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access | SYNCHRONIZE,
            &attributes,
            &mut status_block,
            ptr::null(),
            0,
            share,
            disposition,
            options | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT | FILE_OPEN_FOR_BACKUP_INTENT,
            ptr::null(),
            0,
        )
    };
    if status < 0 {
        // SAFETY: pure conversion function without pointer arguments.
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    owned(handle)
}

fn reject_unsafe(handle: HANDLE, want_dir: bool) -> io::Result<HandleStat> {
    let stat = stat_of(handle)?;
    if stat.attributes & attributes::REPARSE_POINT != 0 {
        return Err(not_safe("Verknüpfung/Reparse Point wird nicht verfolgt"));
    }
    if attributes::is_placeholder(stat.attributes) {
        return Err(not_safe("Cloud-Platzhalter wird nicht geladen"));
    }
    let is_dir = stat.attributes & attributes::DIRECTORY != 0;
    if is_dir != want_dir {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unerwarteter Objekttyp"));
    }
    Ok(stat)
}

// ------------------------------------------------------------ directories

/// A directory opened without following reparse points.
#[derive(Debug)]
pub struct DirHandle {
    handle: OwnedHandle,
}

impl DirHandle {
    /// Opens an approved root by path. With `exclusive`, other processes
    /// cannot rename or delete the root while this handle is held.
    pub fn open_root(path: &Path, exclusive: bool) -> io::Result<Self> {
        let name = wide_long_path(path);
        let share = FILE_SHARE_READ | FILE_SHARE_WRITE | if exclusive { 0 } else { FILE_SHARE_DELETE };
        // SAFETY: `name` is NUL-terminated and alive for the call; no
        // security attributes or template handle are passed.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
                share,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        let dir = Self { handle: owned(handle)? };
        reject_unsafe(raw(&dir.handle), true)?;
        Ok(dir)
    }

    pub fn stat(&self) -> io::Result<HandleStat> {
        stat_of(raw(&self.handle))
    }

    pub fn identity(&self) -> io::Result<FileIdentity> {
        identity_of(raw(&self.handle))
    }

    /// Where this directory really is, as the system resolves it now: drive
    /// letter and real names, links and SUBST drives resolved. A path on a
    /// network share keeps its `\\server\share` form.
    pub fn final_path(&self) -> io::Result<String> {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
        };
        let mut buffer = vec![0_u16; 1024];
        // SAFETY: the buffer is writable for its stated length; the handle is open.
        let len = unsafe {
            GetFinalPathNameByHandleW(
                raw(&self.handle),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        if len >= buffer.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Pfad zu lang"));
        }
        let text = String::from_utf16(&buffer[..len]).map_err(|_| io::Error::other("Pfad nicht darstellbar"))?;
        Ok(match text.strip_prefix(r"\\?\UNC\") {
            Some(share) => format!(r"\\{share}"),
            None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
        })
    }

    /// Lists entries (without `.`/`..`). Returns `true` as second value when
    /// `max` was reached and the listing is incomplete.
    pub fn entries(&self, max: usize) -> io::Result<(Vec<DirEntryInfo>, bool)> {
        let mut buffer = vec![0_u64; 8 * 1024];
        let byte_len = (buffer.len() * size_of::<u64>()) as u32;
        let mut class = FileIdExtdDirectoryRestartInfo;
        let mut out = Vec::new();
        loop {
            // SAFETY: the buffer is 8-byte aligned, writable and `byte_len`
            // bytes long; the handle has FILE_LIST_DIRECTORY access.
            let ok =
                unsafe { GetFileInformationByHandleEx(raw(&self.handle), class, buffer.as_mut_ptr().cast(), byte_len) };
            if ok == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    return Ok((out, false));
                }
                return Err(error);
            }
            class = FileIdExtdDirectoryInfo;
            let base = buffer.as_ptr().cast::<u8>();
            let mut offset = 0_usize;
            loop {
                // SAFETY: the kernel wrote a chain of records into `buffer`;
                // `offset` follows `NextEntryOffset` and stays inside it. The
                // header is copied with an unaligned read.
                let header = unsafe { ptr::read_unaligned(base.add(offset).cast::<FILE_ID_EXTD_DIR_INFO>()) };
                let name_units = header.FileNameLength as usize / 2;
                // SAFETY: the name follows the header inside the same record.
                let name = unsafe {
                    let start = base.add(offset + offset_of!(FILE_ID_EXTD_DIR_INFO, FileName)).cast::<u16>();
                    let mut units = vec![0_u16; name_units];
                    ptr::copy_nonoverlapping(start, units.as_mut_ptr(), name_units);
                    OsString::from_wide(&units)
                };
                if name != "." && name != ".." {
                    if out.len() >= max {
                        return Ok((out, true));
                    }
                    out.push(DirEntryInfo {
                        name,
                        attributes: header.FileAttributes,
                        size: header.EndOfFile.max(0) as u64,
                        modified: filetime_to_unix(header.LastWriteTime),
                    });
                }
                if header.NextEntryOffset == 0 {
                    break;
                }
                offset += header.NextEntryOffset as usize;
            }
        }
    }

    /// Opens a child directory relative to this handle; never follows links.
    pub fn open_dir(&self, name: &OsStr) -> io::Result<DirHandle> {
        self.child_dir(name, SHARE_ALL)
    }

    /// Like [`Self::open_dir`], but while the handle is held nobody can
    /// rename or delete that directory. Its children can still be moved.
    pub fn open_dir_pinned(&self, name: &OsStr) -> io::Result<DirHandle> {
        self.child_dir(name, FILE_SHARE_READ | FILE_SHARE_WRITE)
    }

    /// Opens a child directory pinned like [`Self::open_dir_pinned`],
    /// creating it first if it does not exist. A link at that name is
    /// opened as itself and refused, never followed.
    pub fn create_dir_pinned(&self, name: &OsStr) -> io::Result<DirHandle> {
        use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF;
        let handle = create_relative(
            raw(&self.handle),
            name,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE,
            FILE_DIRECTORY_FILE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_OPEN_IF,
        )?;
        reject_unsafe(raw(&handle), true)?;
        Ok(DirHandle { handle })
    }

    /// Replaces the file `name` in this directory with `contents`. They go to
    /// a new temporary file created here (creation fails if anything, a link
    /// included, already has that name), which is then renamed over `name`
    /// within this directory: the old entry is replaced, never followed. An
    /// existing `name` that is a link or has further hard links is refused.
    /// On any failure the temporary file is removed again.
    pub fn replace_file(&self, name: &OsStr, contents: &[u8]) -> io::Result<()> {
        use windows_sys::Wdk::Storage::FileSystem::{
            FILE_CREATE, FILE_RENAME_INFORMATION, FileRenameInformation, FileRenameInformationEx, NtSetInformationFile,
        };
        use windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE;
        use windows_sys::Win32::System::WindowsProgramming::{
            FILE_RENAME_FLAG_POSIX_SEMANTICS, FILE_RENAME_FLAG_REPLACE_IF_EXISTS,
        };
        match open_relative(raw(&self.handle), name, FILE_READ_ATTRIBUTES, FILE_NON_DIRECTORY_FILE, SHARE_ALL) {
            Ok(existing) => {
                let stat = stat_of(raw(&existing))?;
                if stat.attributes & attributes::REPARSE_POINT != 0 {
                    return Err(not_safe("Ziel ist eine Verknüpfung"));
                }
                let standard: FILE_STANDARD_INFO = handle_info(raw(&existing), FileStandardInfo)?;
                if standard.NumberOfLinks > 1 {
                    return Err(not_safe("Ziel hat weitere Hardlinks"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let mut temp_name = name.to_os_string();
        temp_name.push(format!(".mogumogu-{}-{stamp}", std::process::id()));
        let temp = create_relative(
            raw(&self.handle),
            &temp_name,
            FILE_GENERIC_WRITE | DELETE,
            FILE_NON_DIRECTORY_FILE,
            0,
            FILE_CREATE,
        )?;
        // SAFETY: ownership of the valid handle moves into the `File`.
        let mut file = unsafe { File::from_raw_handle(temp.into_raw_handle()) };
        let result = (|| -> io::Result<()> {
            file.write_all(contents)?;
            file.sync_all()?;
            // The name is NUL-terminated; the NUL counts in the buffer size,
            // not in `FileNameLength`.
            let units: Vec<u16> = name.encode_wide().chain(std::iter::once(0)).collect();
            let header = offset_of!(FILE_RENAME_INFORMATION, FileName);
            let size = header + units.len() * size_of::<u16>();
            let mut buffer = vec![0_u64; size.div_ceil(size_of::<u64>()) + 1];
            let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
            // SAFETY: `buffer` is 8-byte aligned and large enough for the
            // header plus the name; every field is written before use.
            unsafe {
                (*info).Anonymous.Flags = FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS;
                (*info).RootDirectory = raw(&self.handle);
                (*info).FileNameLength = ((units.len() - 1) * size_of::<u16>()) as u32;
                ptr::copy_nonoverlapping(
                    units.as_ptr(),
                    buffer.as_mut_ptr().cast::<u8>().add(header).cast::<u16>(),
                    units.len(),
                );
            }
            let rename = |class, info: *const FILE_RENAME_INFORMATION| -> u32 {
                // SAFETY: zeroed is a valid initial state for the status block.
                let mut status_block: IO_STATUS_BLOCK = unsafe { zeroed() };
                // SAFETY: `info` points into `buffer`, alive for the call, with
                // `size` valid bytes; the handle carries DELETE access. The bare
                // name resolves below `RootDirectory`, this directory's handle.
                let status = unsafe {
                    NtSetInformationFile(file.as_raw_handle(), &mut status_block, info.cast(), size as u32, class)
                };
                // SAFETY: pure conversion function without pointer arguments.
                if status < 0 { unsafe { RtlNtStatusToDosError(status) } } else { 0 }
            };
            let code = rename(FileRenameInformationEx, info);
            if code == 0 {
                return Ok(());
            }
            if ![ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_INVALID_FUNCTION].contains(&code) {
                return Err(io::Error::from_raw_os_error(code as i32));
            }
            // Older Windows 10 builds: the classic class replaces as well.
            // SAFETY: as above; the union's first byte now reads as `true`.
            unsafe { (*info).Anonymous.Flags = FILE_RENAME_FLAG_REPLACE_IF_EXISTS };
            match rename(FileRenameInformation, info) {
                0 => Ok(()),
                code => Err(io::Error::from_raw_os_error(code as i32)),
            }
        })();
        if result.is_err() {
            let discard = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: valid handle with DELETE access; `discard` is a valid
            // record of the declared class and size.
            unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FileDispositionInfo,
                    (&raw const discard).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                );
            }
        }
        result
    }

    fn child_dir(&self, name: &OsStr, share: u32) -> io::Result<DirHandle> {
        let handle = open_relative(
            raw(&self.handle),
            name,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE,
            FILE_DIRECTORY_FILE,
            share,
        )?;
        reject_unsafe(raw(&handle), true)?;
        Ok(DirHandle { handle })
    }

    /// Opens a child file for reading relative to this handle.
    pub fn open_file(&self, name: &OsStr) -> io::Result<(File, HandleStat)> {
        let handle = open_relative(raw(&self.handle), name, FILE_GENERIC_READ, FILE_NON_DIRECTORY_FILE, SHARE_ALL)?;
        let stat = reject_unsafe(raw(&handle), false)?;
        // SAFETY: ownership of the valid handle moves into the `File`.
        Ok((unsafe { File::from_raw_handle(handle.into_raw_handle()) }, stat))
    }

    /// Whether a child currently exists (does not follow links).
    pub fn child_identity(&self, name: &OsStr, directory: bool) -> io::Result<Option<FileIdentity>> {
        let options = if directory { FILE_DIRECTORY_FILE } else { FILE_NON_DIRECTORY_FILE };
        match open_relative(raw(&self.handle), name, FILE_READ_ATTRIBUTES, options, SHARE_ALL) {
            Ok(handle) => identity_of(raw(&handle)).map(Some),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Identity of an existing file or directory reached by path, without
/// following a final reparse point (which is rejected).
pub fn path_identity(path: &Path) -> io::Result<FileIdentity> {
    let name = wide_long_path(path);
    // SAFETY: NUL-terminated path, no optional pointers.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    let handle = owned(handle)?;
    let stat = stat_of(raw(&handle))?;
    if stat.attributes & attributes::REPARSE_POINT != 0 {
        return Err(not_safe("Verknüpfung/Reparse Point wird nicht als Freigabeziel akzeptiert"));
    }
    Ok(stat.identity)
}

// -------------------------------------------------------------- processes

/// Starts only the sibling owner, without inheriting any caller handles.
/// In particular a background owner must not keep captured CLI pipes open.
pub fn spawn_owner(exe: &Path, data_dir: Option<&Path>, demo: bool) -> io::Result<()> {
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    };
    let mut command = owner_command_line(exe, data_dir, demo)?;
    command.push(0);
    if command.len() > 32767 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Besitzeraufruf zu lang"));
    }
    let executable = wide(exe.as_os_str());
    // SAFETY: STARTUPINFOW is a C output/input record; zero represents no
    // optional desktop, title, standard handles or window-position overrides.
    let mut startup: STARTUPINFOW = unsafe { zeroed() };
    startup.cb = size_of::<STARTUPINFOW>() as u32;
    // SAFETY: PROCESS_INFORMATION is a C output record; CreateProcessW fills
    // the handles and identifiers on success. Zero initialization is valid.
    let mut info: PROCESS_INFORMATION = unsafe { zeroed() };
    // SAFETY: executable and command are NUL-terminated, owned UTF-16 buffers;
    // command is writable for the duration of this call. Both structures live
    // through the call. FALSE disables all handle inheritance; no elevation,
    // shell, alternate user, custom environment or security descriptor is used.
    let created = unsafe {
        CreateProcessW(
            executable.as_ptr(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            ptr::null(),
            ptr::null(),
            &startup,
            &mut info,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    let _process = owned(info.hProcess)?;
    let _thread = owned(info.hThread)?;
    Ok(())
}

/// `"…\mogumogu.exe" --tray [--demo] [--data-dir "…"]`, without NUL. Shared by
/// owner start and the per-user start entry, so both quote identically.
fn owner_command_line(exe: &Path, data_dir: Option<&Path>, demo: bool) -> io::Result<Vec<u16>> {
    if !exe.is_absolute()
        || exe.file_name() != Some(OsStr::new("mogumogu.exe"))
        || data_dir.is_some_and(|p| !p.is_absolute())
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Absoluter Besitzerpfad erforderlich"));
    }
    let mut args = vec![exe.as_os_str(), OsStr::new("--tray")];
    if demo {
        args.push(OsStr::new("--demo"));
    }
    if let Some(dir) = data_dir {
        args.extend([OsStr::new("--data-dir"), dir.as_os_str()]);
    }
    let mut command = Vec::new();
    for arg in args {
        if !command.is_empty() {
            command.push(b' ' as u16);
        }
        command.extend(quote_owner_arg(arg)?);
    }
    Ok(command)
}

fn quote_owner_arg(arg: &OsStr) -> io::Result<Vec<u16>> {
    let input = arg.encode_wide().collect::<Vec<_>>();
    if input.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL im Besitzerargument"));
    }
    let mut out = vec![b'"' as u16];
    let mut slashes = 0;
    for unit in input {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(b'\\' as u16, if unit == b'"' as u16 { slashes * 2 + 1 } else { slashes }));
        out.push(unit);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    Ok(out)
}

fn open_process(pid: u32) -> io::Result<Option<OwnedHandle>> {
    // SAFETY: plain call; a null result is handled below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        return match error.raw_os_error().map(|c| c as u32) {
            Some(ERROR_INVALID_PARAMETER) => Ok(None),
            _ => Err(error),
        };
    }
    owned(handle).map(Some)
}

// ------------------------------------------------------ users and sessions

fn token_user_sid(process: HANDLE) -> io::Result<Vec<u8>> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: writable out-handle; `process` is a valid process handle.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = owned(token)?;
    let mut needed = 0_u32;
    // SAFETY: size query with a null buffer is the documented pattern.
    unsafe { GetTokenInformation(raw(&token), TokenUser, ptr::null_mut(), 0, &mut needed) };
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8).max(1)];
    // SAFETY: the buffer is at least `needed` bytes and 8-byte aligned.
    let ok = unsafe { GetTokenInformation(raw(&token), TokenUser, buffer.as_mut_ptr().cast(), needed, &mut needed) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // The SID lives inside `buffer`; copy it out as raw bytes (length from
    // the SID header: 8 + 4 * SubAuthorityCount).
    // SAFETY: TOKEN_USER is at the start of the buffer written above.
    let sid = unsafe { (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid }.cast::<u8>();
    // SAFETY: a valid SID has at least 8 bytes; byte 1 is the sub-authority count.
    let len = 8 + 4 * usize::from(unsafe { *sid.add(1) });
    // SAFETY: the SID is fully contained in `buffer`.
    Ok(unsafe { std::slice::from_raw_parts(sid, len) }.to_vec())
}

fn sid_string(sid: &[u8]) -> io::Result<String> {
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: `sid` is a complete SID; the out pointer is freed with LocalFree.
    if unsafe { ConvertSidToStringSidW(sid.as_ptr() as *mut c_void, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the returned string is NUL-terminated.
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    // SAFETY: `len` units are readable; afterwards the buffer is released.
    let result = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    // SAFETY: memory allocated by the API above.
    unsafe { LocalFree(text as HLOCAL) };
    Ok(result)
}

pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: pseudo handle, no cleanup needed.
    sid_string(&token_user_sid(unsafe { GetCurrentProcess() })?)
}

fn session_of(pid: u32) -> io::Result<u32> {
    let mut session = 0_u32;
    // SAFETY: writable out-parameter.
    if unsafe { ProcessIdToSessionId(pid, &mut session) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(session)
}

pub fn current_session_id() -> io::Result<u32> {
    // SAFETY: trivial getter.
    session_of(unsafe { GetCurrentProcessId() })
}

fn same_user(pid: u32) -> io::Result<bool> {
    let Some(process) = open_process(pid)? else { return Ok(false) };
    let theirs = token_user_sid(raw(&process))?;
    // SAFETY: pseudo handle.
    let ours = token_user_sid(unsafe { GetCurrentProcess() })?;
    // SAFETY: both buffers contain complete SIDs.
    Ok(unsafe { EqualSid(theirs.as_ptr() as *mut c_void, ours.as_ptr() as *mut c_void) } != 0)
}

// ------------------------------------------------------------ named pipes

/// Server end of the owner IPC pipe: current user only, local clients only,
/// first instance only (prevents pipe squatting by another process).
#[derive(Debug)]
pub struct PipeListener {
    handle: OwnedHandle,
    session: u32,
}

impl PipeListener {
    pub fn bind(name: &str) -> io::Result<Self> {
        let sddl = wide(OsStr::new(&format!("D:P(A;;GA;;;{})", current_user_sid()?)));
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: NUL-terminated SDDL; descriptor is freed below.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let name = wide(OsStr::new(name));
        // SAFETY: NUL-terminated name and a valid security descriptor that
        // stays alive until after the call.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64 * 1024,
                64 * 1024,
                0,
                &security,
            )
        };
        // SAFETY: descriptor allocated by the conversion API above.
        unsafe { LocalFree(descriptor as HLOCAL) };
        Ok(Self { handle: owned(handle)?, session: current_session_id()? })
    }

    /// Blocks until a client connects. Clients from another logon session
    /// are disconnected without reading their request.
    pub fn accept(&self) -> io::Result<PipeConnection<'_>> {
        // SAFETY: valid pipe handle, synchronous mode (no OVERLAPPED).
        let ok = unsafe { ConnectNamedPipe(raw(&self.handle), ptr::null_mut()) };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
                // A client that connected and closed before this call (the
                // CLI's liveness probe right after the owner started) leaves
                // the instance closing: every later ConnectNamedPipe fails
                // with ERROR_NO_DATA and clients only see ERROR_PIPE_BUSY.
                // Disconnecting makes the instance listen again.
                // SAFETY: the listener owns this valid pipe handle.
                unsafe { DisconnectNamedPipe(raw(&self.handle)) };
                return Err(error);
            }
        }
        let connection = PipeConnection { handle: raw(&self.handle), _listener: PhantomData };
        let mut pid = 0_u32;
        // SAFETY: connected pipe, writable out-parameter.
        if unsafe { GetNamedPipeClientProcessId(raw(&self.handle), &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if session_of(pid)? != self.session {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Client aus anderer Anmeldesitzung abgewiesen",
            ));
        }
        Ok(connection)
    }
}

/// One connected client; disconnects on drop so the instance can be reused.
#[derive(Debug)]
pub struct PipeConnection<'a> {
    handle: HANDLE,
    _listener: PhantomData<&'a PipeListener>,
}

impl Read for PipeConnection<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut read = 0_u32;
        let len = buf.len().min(u32::MAX as usize) as u32;
        // SAFETY: writable buffer of `len` bytes; synchronous handle.
        let ok = unsafe { ReadFile(self.handle, buf.as_mut_ptr(), len, &mut read, ptr::null_mut()) };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) {
                return Ok(read as usize);
            }
            return Err(error);
        }
        Ok(read as usize)
    }
}

impl Write for PipeConnection<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written = 0_u32;
        let len = buf.len().min(u32::MAX as usize) as u32;
        // SAFETY: readable buffer of `len` bytes; synchronous handle.
        if unsafe { WriteFile(self.handle, buf.as_ptr(), len, &mut written, ptr::null_mut()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(written as usize)
    }
    fn flush(&mut self) -> io::Result<()> {
        // SAFETY: valid handle.
        unsafe { FlushFileBuffers(self.handle) };
        Ok(())
    }
}

impl Drop for PipeConnection<'_> {
    fn drop(&mut self) {
        // SAFETY: the listener owns the handle and outlives this borrow.
        unsafe {
            FlushFileBuffers(self.handle);
            DisconnectNamedPipe(self.handle);
        }
    }
}

/// Whether `pipe_connect` gave up because every instance stayed busy.
pub fn pipe_busy(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
}

/// Connects to the owner pipe. `Ok(None)` means no owner is running. The
/// server process must belong to the same user, otherwise the connection
/// is refused (a foreign process could have created the name first).
pub fn pipe_connect(name: &str, timeout: Duration) -> io::Result<Option<File>> {
    let name = wide(OsStr::new(name));
    let deadline = Instant::now() + timeout;
    loop {
        // SAFETY: NUL-terminated name. SECURITY_IDENTIFICATION prevents the
        // server from impersonating this client beyond identification.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            let handle = owned(handle)?;
            let mut pid = 0_u32;
            // SAFETY: connected client end, writable out-parameter.
            if unsafe { GetNamedPipeServerProcessId(raw(&handle), &mut pid) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if !same_user(pid)? {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Pipe gehört einem fremden Prozess"));
            }
            // SAFETY: ownership moves into the File.
            return Ok(Some(unsafe { File::from_raw_handle(handle.into_raw_handle()) }));
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error().map(|c| c as u32) {
            Some(ERROR_FILE_NOT_FOUND) => return Ok(None),
            Some(ERROR_PIPE_BUSY) if Instant::now() < deadline => {
                let remaining = deadline.saturating_duration_since(Instant::now()).as_millis().min(5_000) as u32;
                // SAFETY: NUL-terminated name.
                unsafe { WaitNamedPipeW(name.as_ptr(), remaining.max(1)) };
            }
            _ => return Err(error),
        }
    }
}

// ---------------------------------------------------------------- registry

struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: key opened by RegOpenKeyExW and closed exactly once.
        unsafe { RegCloseKey(self.0) };
    }
}

fn reg_open(parent: HKEY, path: &str, extra: u32) -> Option<RegKey> {
    let path = wide(OsStr::new(path));
    let mut key: HKEY = ptr::null_mut();
    // SAFETY: NUL-terminated subkey path, writable out-key.
    let status = unsafe { RegOpenKeyExW(parent, path.as_ptr(), 0, KEY_READ | extra, &mut key) };
    (status == 0).then_some(RegKey(key))
}

fn reg_string(key: &RegKey, value: &str) -> Option<String> {
    let value = wide(OsStr::new(value));
    let mut bytes = 0_u32;
    // SAFETY: size query with a null buffer.
    let status = unsafe {
        RegGetValueW(key.0, ptr::null(), value.as_ptr(), RRF_RT_REG_SZ, ptr::null_mut(), ptr::null_mut(), &mut bytes)
    };
    if status != 0 || bytes == 0 || bytes > 64 * 1024 {
        return None;
    }
    let mut buffer = vec![0_u16; bytes as usize / 2 + 1];
    // SAFETY: buffer has at least `bytes` bytes.
    let status = unsafe {
        RegGetValueW(
            key.0,
            ptr::null(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 {
        return None;
    }
    let text = String::from_utf16_lossy(&buffer[..(bytes as usize / 2)]);
    let text = text.trim_end_matches('\0').trim().to_string();
    (!text.is_empty()).then_some(text)
}

// ------------------------------------------------------------ recycle bin

/// Moves one folder to the recycle bin of its drive, by path. Local fixed
/// drives only: elsewhere the shell has no recycle bin and would delete for
/// good. If the folder is too large for the bin, the shell asks the person
/// before deleting it permanently; nothing is destroyed silently.
pub fn recycle(path: &Path) -> io::Result<()> {
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };
    if !fixed_disk(path) {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "kein Papierkorb auf diesem Laufwerk"));
    }
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    if from.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL im Pfad"));
    }
    // A list with exactly one entry, terminated by two NULs.
    from.extend([0, 0]);
    let flags = FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_NOERRORUI | FOF_SILENT;
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: ptr::null(),
        fFlags: flags as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: ptr::null_mut(),
        lpszProgressTitle: ptr::null(),
    };
    // SAFETY: `from` is double NUL-terminated and outlives the synchronous
    // call; the other pointers are null as the API allows for FO_DELETE.
    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(io::Error::other(format!("Papierkorb meldet Fehler 0x{result:x}")));
    }
    if operation.fAnyOperationsAborted != 0 {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "im Dialog abgebrochen"));
    }
    Ok(())
}

/// A plain `X:\…` path on a fixed disk; RAM disks have no recycle bin.
pub fn fixed_disk(path: &Path) -> bool {
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;
    let text = path.as_os_str().to_string_lossy();
    let bytes = text.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return false;
    }
    let root = wide(OsStr::new(&text[..3]));
    // SAFETY: NUL-terminated drive root such as `C:\`; the call only reads it.
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_FIXED }
}

// ------------------------------------------------------- shell and system

/// Only local fixed drives are inspected by the system check: network and
/// removable media can block, wake devices or ask for credentials.
pub fn local_fixed_path(path: &Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_RAMDISK};
    let text = path.as_os_str().to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let bytes = text.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return false;
    }
    let root = wide(OsStr::new(&text[..3]));
    // SAFETY: NUL-terminated drive root such as `C:\`; the call only reads it.
    let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
    matches!(kind, DRIVE_FIXED | DRIVE_RAMDISK)
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "mogumogu";

/// The per-user start entry named `mogumogu`, if present. Another program
/// may have written it; callers compare it with [`autostart_command`].
pub fn autostart_entry() -> io::Result<Option<String>> {
    Ok(reg_open(HKEY_CURRENT_USER, RUN_KEY, 0).and_then(|key| reg_string(&key, RUN_VALUE)))
}

/// The entry this owner would register: itself, in the tray, same data.
pub fn autostart_command(exe: &Path, data_dir: Option<&Path>, demo: bool) -> io::Result<String> {
    Ok(String::from_utf16_lossy(&owner_command_line(exe, data_dir, demo)?))
}

/// Removes the per-user start entry named `mogumogu`. Callers first check
/// that it starts exactly this owner; nothing is ever written there.
pub fn remove_autostart() -> io::Result<()> {
    use windows_sys::Win32::System::Registry::{KEY_SET_VALUE, RegDeleteValueW};
    let key = reg_open(HKEY_CURRENT_USER, RUN_KEY, KEY_SET_VALUE)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Startschlüssel des Benutzers nicht erreichbar"))?;
    let name = wide(OsStr::new(RUN_VALUE));
    // SAFETY: opened key with KEY_SET_VALUE; NUL-terminated value name.
    let status = match unsafe { RegDeleteValueW(key.0, name.as_ptr()) } {
        ERROR_FILE_NOT_FOUND => 0,
        other => other,
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

/// Native folder dialog owned by the foreground window. Returns the chosen
/// file-system folder or `None` when cancelled. Nothing is read or created.
pub fn pick_folder(title: &str) -> io::Result<Option<String>> {
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize};
    use windows_sys::Win32::UI::Shell::{
        BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW, SHGetPathFromIDListW,
    };
    // SAFETY: plain COM initialisation of the calling UI thread; a positive
    // result (S_OK or S_FALSE) is balanced by CoUninitialize below.
    let initialized = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0;
    let title = wide(OsStr::new(title));
    let mut display = [0_u16; 260];
    let owner = own_window();
    let info = BROWSEINFOW {
        hwndOwner: owner,
        pidlRoot: ptr::null_mut(),
        pszDisplayName: display.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    // SAFETY: `info` and its buffers outlive this modal call; no callback.
    let pidl = unsafe { SHBrowseForFolderW(&info) };
    let result = if pidl.is_null() {
        Ok(None)
    } else {
        let mut buffer = [0_u16; 260];
        // SAFETY: valid ID list from the dialog; MAX_PATH-sized out buffer.
        let ok = unsafe { SHGetPathFromIDListW(pidl, buffer.as_mut_ptr()) };
        // SAFETY: the dialog allocated the ID list with the COM allocator.
        unsafe { CoTaskMemFree(pidl.cast()) };
        let end = buffer.iter().position(|u| *u == 0).unwrap_or(buffer.len());
        if ok == 0 || end == 0 {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "Kein Dateisystemordner gewählt"))
        } else {
            Ok(Some(String::from_utf16_lossy(&buffer[..end])))
        }
    };
    if initialized {
        // SAFETY: balances the successful CoInitializeEx above on this thread.
        unsafe { CoUninitialize() };
    }
    result
}

// ------------------------------------------------------------------- misc

pub fn local_utc_offset_seconds() -> i64 {
    // SAFETY: zeroed is valid for this plain struct; writable out-parameter.
    let mut info: TIME_ZONE_INFORMATION = unsafe { zeroed() };
    // SAFETY: as above.
    let mode = unsafe { GetTimeZoneInformation(&mut info) };
    let bias = match mode {
        TIME_ZONE_ID_STANDARD => info.Bias + info.StandardBias,
        TIME_ZONE_ID_DAYLIGHT => info.Bias + info.DaylightBias,
        _ => info.Bias,
    };
    -i64::from(bias) * 60
}

/// The calling UI thread's visible captioned window (the dashboard). A modal
/// dialog must never belong to another application's foreground window: it
/// would disable that window, or fail when the window is not this thread's,
/// which happens when the action comes from a shortcut, the tray or
/// assistive technology while mogumogu is not in front.
fn own_window() -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GWL_STYLE, GetWindowLongW, IsWindowVisible, WS_CAPTION,
    };
    unsafe extern "system" fn first_dashboard(hwnd: HWND, found: LPARAM) -> windows_sys::core::BOOL {
        // SAFETY: `hwnd` comes from the enumeration and is a live window.
        let visible = unsafe { IsWindowVisible(hwnd) } != 0;
        // SAFETY: as above; reading the style has no side effects.
        let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
        if visible && style & WS_CAPTION == WS_CAPTION {
            // SAFETY: `found` is the address of the HWND in `own_window`,
            // alive for the whole synchronous enumeration.
            unsafe { *(found as *mut HWND) = hwnd };
            return 0;
        }
        1
    }
    let mut found: HWND = ptr::null_mut();
    // SAFETY: the callback only writes `found`, which outlives this
    // synchronous call on the current thread.
    unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(first_dashboard), &mut found as *mut HWND as LPARAM) };
    found
}

/// Modal error box for GUI builds without a console.
pub fn show_error(title: &str, message: &str) {
    let text = wide(OsStr::new(message));
    let caption = wide(OsStr::new(title));
    // SAFETY: both buffers are NUL-terminated and alive during this
    // synchronous call; a null owner window is allowed.
    unsafe { MessageBoxW(ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR) };
}

#[cfg(test)]
mod owner_arguments {
    use super::*;
    #[test]
    fn windows_quoting_preserves_spaces_quotes_and_final_backslashes() {
        let quoted = quote_owner_arg(OsStr::new("a\"b\\")).unwrap();
        assert_eq!(String::from_utf16(&quoted).unwrap(), r#""a\"b\\""#);
        let quoted = quote_owner_arg(OsStr::new(r"C:\data folder\")).unwrap();
        assert_eq!(String::from_utf16(&quoted).unwrap(), r#""C:\data folder\\""#);
        assert!(quote_owner_arg(OsStr::new("bad\0arg")).is_err());
    }
}
