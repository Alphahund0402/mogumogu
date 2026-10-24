//! Non-Windows stubs so the core compiles for documentation and analysis.
//! mogumogu targets Windows 10/11; these never pretend to provide the
//! Windows safety guarantees and report `Unsupported` instead.
use super::{DirEntryInfo, FileIdentity, HandleStat};
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;

fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "nur unter Windows verfügbar"))
}

pub fn spawn_owner(_: &Path, _: Option<&Path>, _: bool) -> io::Result<()> {
    unsupported()
}

pub fn logical_drives() -> io::Result<Vec<crate::domain::DriveUsage>> {
    unsupported()
}

#[derive(Debug)]
pub struct DirHandle;

impl DirHandle {
    pub fn open_root(_: &Path, _: bool) -> io::Result<Self> {
        unsupported()
    }
    pub fn stat(&self) -> io::Result<HandleStat> {
        unsupported()
    }
    pub fn identity(&self) -> io::Result<FileIdentity> {
        unsupported()
    }
    pub fn final_path(&self) -> io::Result<String> {
        unsupported()
    }
    pub fn entries(&self, _: usize) -> io::Result<(Vec<DirEntryInfo>, bool)> {
        unsupported()
    }
    pub fn open_dir(&self, _: &OsStr) -> io::Result<DirHandle> {
        unsupported()
    }
    pub fn open_dir_pinned(&self, _: &OsStr) -> io::Result<DirHandle> {
        unsupported()
    }
    pub fn create_dir_pinned(&self, _: &OsStr) -> io::Result<DirHandle> {
        unsupported()
    }
    pub fn replace_file(&self, _: &OsStr, _: &[u8]) -> io::Result<()> {
        unsupported()
    }
    pub fn open_file(&self, _: &OsStr) -> io::Result<(File, HandleStat)> {
        unsupported()
    }
    pub fn child_identity(&self, _: &OsStr, _: bool) -> io::Result<Option<FileIdentity>> {
        unsupported()
    }
}

pub fn path_identity(_: &Path) -> io::Result<FileIdentity> {
    unsupported()
}
pub fn recycle(_: &Path) -> io::Result<()> {
    unsupported()
}
pub fn fixed_disk(_: &Path) -> bool {
    false
}

pub fn current_user_sid() -> io::Result<String> {
    unsupported()
}
pub fn current_session_id() -> io::Result<u32> {
    unsupported()
}

#[derive(Debug)]
pub struct PipeListener;

#[derive(Debug)]
pub struct PipeConnection<'a>(std::marker::PhantomData<&'a ()>);

impl PipeListener {
    pub fn bind(_: &str) -> io::Result<Self> {
        unsupported()
    }
    pub fn accept(&self) -> io::Result<PipeConnection<'_>> {
        unsupported()
    }
}

impl io::Read for PipeConnection<'_> {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        unsupported()
    }
}

impl io::Write for PipeConnection<'_> {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        unsupported()
    }
    fn flush(&mut self) -> io::Result<()> {
        unsupported()
    }
}

pub fn pipe_connect(_: &str, _: Duration) -> io::Result<Option<File>> {
    unsupported()
}
pub fn pipe_busy(_: &io::Error) -> bool {
    false
}
pub fn local_utc_offset_seconds() -> i64 {
    0
}
pub fn show_error(title: &str, message: &str) {
    eprintln!("{title}: {message}");
}
/// Portable core tests only check absolute local paths.
pub fn local_fixed_path(path: &Path) -> bool {
    path.is_absolute()
}
pub fn autostart_entry() -> io::Result<Option<String>> {
    Ok(None)
}
pub fn autostart_command(_: &Path, _: Option<&Path>, _: bool) -> io::Result<String> {
    unsupported()
}
pub fn remove_autostart() -> io::Result<()> {
    unsupported()
}
pub fn pick_folder(_: &str) -> io::Result<Option<String>> {
    Ok(None)
}
