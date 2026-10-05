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
//! - Deletion uses the handle whose identity was just verified.
use super::{DirEntryInfo, FileIdentity, HandleStat, UninstallEntry, attributes, filetime_to_unix};
use crate::domain::ProcessIdentity;
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
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA,
    ERROR_NO_MORE_FILES, ERROR_NOT_SUPPORTED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE,
    HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_BASIC_INFO, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_ID_EXTD_DIR_INFO,
    FILE_ID_INFO, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_STANDARD_INFO, FileBasicInfo, FileDispositionInfo, FileDispositionInfoEx, FileIdExtdDirectoryInfo,
    FileIdExtdDirectoryRestartInfo, FileIdInfo, FileStandardInfo, FlushFileBuffers, GetFileInformationByHandle,
    GetFileInformationByHandleEx, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, SYNCHRONIZE, SetFileInformationByHandle, WriteFile,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JobObjectBasicAccountingInformation, OpenJobObjectW, QueryInformationJobObject,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, RRF_RT_REG_DWORD,
    RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
};
use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

const JOB_OBJECT_QUERY: u32 = 0x4;
const TIME_ZONE_ID_STANDARD: u32 = 1;
const TIME_ZONE_ID_DAYLIGHT: u32 = 2;
const FILE_TRAVERSE: u32 = 0x20;

// ---------------------------------------------------------------- helpers

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
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

/// Opens `name` relative to `parent` without any path re-resolution.
fn open_relative(parent: HANDLE, name: &OsStr, access: u32, options: u32) -> io::Result<OwnedHandle> {
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
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
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
        let handle = open_relative(
            raw(&self.handle),
            name,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE,
            FILE_DIRECTORY_FILE,
        )?;
        reject_unsafe(raw(&handle), true)?;
        Ok(DirHandle { handle })
    }

    /// Opens a child file for reading relative to this handle.
    pub fn open_file(&self, name: &OsStr) -> io::Result<(File, HandleStat)> {
        let handle = open_relative(raw(&self.handle), name, FILE_GENERIC_READ, FILE_NON_DIRECTORY_FILE)?;
        let stat = reject_unsafe(raw(&handle), false)?;
        // SAFETY: ownership of the valid handle moves into the `File`.
        Ok((unsafe { File::from_raw_handle(handle.into_raw_handle()) }, stat))
    }

    /// Opens a child with DELETE access for a verified removal.
    pub fn open_for_delete(&self, name: &OsStr, directory: bool) -> io::Result<DeleteHandle> {
        let options = if directory { FILE_DIRECTORY_FILE } else { FILE_NON_DIRECTORY_FILE };
        let handle = open_relative(raw(&self.handle), name, DELETE | FILE_READ_ATTRIBUTES, options)?;
        let stat = stat_of(raw(&handle))?;
        if stat.attributes & attributes::REPARSE_POINT != 0 {
            return Err(not_safe("Reparse Point wird nicht entfernt"));
        }
        Ok(DeleteHandle { handle, stat })
    }

    /// Whether a child currently exists (does not follow links).
    pub fn child_identity(&self, name: &OsStr, directory: bool) -> io::Result<Option<FileIdentity>> {
        let options = if directory { FILE_DIRECTORY_FILE } else { FILE_NON_DIRECTORY_FILE };
        match open_relative(raw(&self.handle), name, FILE_READ_ATTRIBUTES, options) {
            Ok(handle) => identity_of(raw(&handle)).map(Some),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// A handle opened with DELETE access whose identity was read on open.
#[derive(Debug)]
pub struct DeleteHandle {
    handle: OwnedHandle,
    stat: HandleStat,
}

impl DeleteHandle {
    pub fn stat(&self) -> HandleStat {
        self.stat
    }

    /// Marks exactly this object for deletion; it disappears on close.
    pub fn delete(self) -> io::Result<()> {
        let info =
            FILE_DISPOSITION_INFO_EX { Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS };
        // SAFETY: `info` is a valid record of the declared class and size.
        let ok = unsafe {
            SetFileInformationByHandle(
                raw(&self.handle),
                FileDispositionInfoEx,
                (&raw const info).cast(),
                size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            let code = error.raw_os_error().unwrap_or_default() as u32;
            if ![ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_INVALID_FUNCTION].contains(&code) {
                return Err(error);
            }
            // Older Windows 10 builds or FAT: classic delete-on-close.
            let legacy = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: as above, with the legacy record type.
            let ok = unsafe {
                SetFileInformationByHandle(
                    raw(&self.handle),
                    FileDispositionInfo,
                    (&raw const legacy).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        drop(self.handle);
        Ok(())
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

fn creation_time(handle: HANDLE) -> io::Result<u64> {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    // SAFETY: four writable FILETIME out-parameters.
    let ok = unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let created: windows_sys::Win32::Foundation::FILETIME = created;
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
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

pub fn process_identity(pid: u32) -> io::Result<Option<ProcessIdentity>> {
    let Some(handle) = open_process(pid)? else { return Ok(None) };
    Ok(Some(ProcessIdentity { pid, created: creation_time(raw(&handle))? }))
}

/// Identity of a spawned child, read from its handle (valid even if the
/// child has already exited, so there is no PID-reuse window).
pub fn child_identity(child: &std::process::Child) -> io::Result<ProcessIdentity> {
    Ok(ProcessIdentity { pid: child.id(), created: creation_time(child.as_raw_handle())? })
}

pub fn current_process_identity() -> io::Result<ProcessIdentity> {
    // SAFETY: trivial getter.
    let pid = unsafe { GetCurrentProcessId() };
    process_identity(pid)?.ok_or_else(|| io::Error::other("eigener Prozess nicht gefunden"))
}

/// True only if a process with this PID *and* creation time still runs.
pub fn process_alive(identity: ProcessIdentity) -> io::Result<bool> {
    let Some(handle) = open_process(identity.pid)? else { return Ok(false) };
    if creation_time(raw(&handle))? != identity.created {
        return Ok(false);
    }
    // SAFETY: valid process handle with SYNCHRONIZE access; zero timeout.
    Ok(unsafe { WaitForSingleObject(raw(&handle), 0) } == WAIT_TIMEOUT)
}

// ------------------------------------------------------------ job objects

/// Named job object grouping a managed run and all its descendants. It is
/// created without KILL_ON_JOB_CLOSE: mogumogu never ends user work.
#[derive(Debug)]
pub struct Job {
    handle: OwnedHandle,
}

impl Job {
    pub fn create(name: &str) -> io::Result<Self> {
        let name = wide(OsStr::new(name));
        // SAFETY: NUL-terminated name, default security.
        let handle = unsafe { CreateJobObjectW(ptr::null(), name.as_ptr()) };
        let already = io::Error::last_os_error().raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32);
        let handle = owned(handle)?;
        if already {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "Job-Objekt existiert bereits"));
        }
        Ok(Self { handle })
    }

    pub fn open(name: &str) -> io::Result<Option<Self>> {
        let name = wide(OsStr::new(name));
        // SAFETY: NUL-terminated name.
        let handle = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, name.as_ptr()) };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) {
                return Ok(None);
            }
            return Err(error);
        }
        owned(handle).map(|handle| Some(Self { handle }))
    }

    /// Puts the calling process into the job; children inherit it.
    pub fn assign_current_process(&self) -> io::Result<()> {
        // SAFETY: valid job handle and the current-process pseudo handle.
        if unsafe { AssignProcessToJobObject(raw(&self.handle), GetCurrentProcess()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn active_processes(&self) -> io::Result<u32> {
        // SAFETY: zeroed is valid for this plain struct.
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
        // SAFETY: writable, correctly sized out-buffer of the requested class.
        let ok = unsafe {
            QueryInformationJobObject(
                raw(&self.handle),
                JobObjectBasicAccountingInformation,
                (&raw mut info).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info.ActiveProcesses)
    }
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

fn reg_dword(key: &RegKey, value: &str) -> Option<u32> {
    let value = wide(OsStr::new(value));
    let mut data = 0_u32;
    let mut bytes = 4_u32;
    // SAFETY: 4-byte out buffer for a DWORD.
    let status = unsafe {
        RegGetValueW(
            key.0,
            ptr::null(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            ptr::null_mut(),
            (&raw mut data).cast(),
            &mut bytes,
        )
    };
    (status == 0).then_some(data)
}

/// Reads the per-user and machine "installed programs" lists, read-only.
pub fn uninstall_entries(max: usize) -> io::Result<(Vec<UninstallEntry>, bool)> {
    const PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    let roots: [(&'static str, HKEY, u32); 3] = [
        ("HKCU", HKEY_CURRENT_USER, 0),
        ("HKLM", HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        ("HKLM32", HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
    ];
    let mut out = Vec::new();
    for (hive, root, extra) in roots {
        let Some(list) = reg_open(root, PATH, extra) else { continue };
        for index in 0.. {
            let mut name = [0_u16; 256];
            let mut len = name.len() as u32;
            // SAFETY: writable name buffer of `len` units.
            let status = unsafe {
                RegEnumKeyExW(
                    list.0,
                    index,
                    name.as_mut_ptr(),
                    &mut len,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if status != 0 {
                break;
            }
            if out.len() >= max {
                return Ok((out, true));
            }
            let sub = String::from_utf16_lossy(&name[..len as usize]);
            let Some(key) = reg_open(list.0, &sub, extra) else { continue };
            if reg_dword(&key, "SystemComponent") == Some(1) || reg_string(&key, "ParentKeyName").is_some() {
                continue;
            }
            let Some(display) = reg_string(&key, "DisplayName") else { continue };
            out.push(UninstallEntry {
                hive,
                name: display,
                version: reg_string(&key, "DisplayVersion"),
                publisher: reg_string(&key, "Publisher"),
            });
        }
    }
    Ok((out, false))
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

/// Modal error box for GUI builds without a console.
pub fn show_error(title: &str, message: &str) {
    let text = wide(OsStr::new(message));
    let caption = wide(OsStr::new(title));
    // SAFETY: both buffers are NUL-terminated and alive during this
    // synchronous call; a null owner window is allowed.
    unsafe { MessageBoxW(ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONERROR) };
}
