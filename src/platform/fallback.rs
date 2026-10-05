//! Non-Windows stubs so the core compiles for documentation and analysis.
//! mogumogu targets Windows 10/11; these never pretend to provide the
//! Windows safety guarantees and report `Unsupported` instead.
use super::{DirEntryInfo, FileIdentity, HandleStat, UninstallEntry};
use crate::domain::ProcessIdentity;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;

fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "nur unter Windows verfügbar"))
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
    pub fn entries(&self, _: usize) -> io::Result<(Vec<DirEntryInfo>, bool)> {
        unsupported()
    }
    pub fn open_dir(&self, _: &OsStr) -> io::Result<DirHandle> {
        unsupported()
    }
    pub fn open_file(&self, _: &OsStr) -> io::Result<(File, HandleStat)> {
        unsupported()
    }
    pub fn open_for_delete(&self, _: &OsStr, _: bool) -> io::Result<DeleteHandle> {
        unsupported()
    }
    pub fn child_identity(&self, _: &OsStr, _: bool) -> io::Result<Option<FileIdentity>> {
        unsupported()
    }
}

#[derive(Debug)]
pub struct DeleteHandle;

impl DeleteHandle {
    pub fn stat(&self) -> HandleStat {
        unreachable!("DeleteHandle cannot be constructed on this platform")
    }
    pub fn delete(self) -> io::Result<()> {
        unsupported()
    }
}

pub fn path_identity(_: &Path) -> io::Result<FileIdentity> {
    unsupported()
}
pub fn process_identity(_: u32) -> io::Result<Option<ProcessIdentity>> {
    unsupported()
}
pub fn child_identity(_: &std::process::Child) -> io::Result<ProcessIdentity> {
    unsupported()
}
pub fn current_process_identity() -> io::Result<ProcessIdentity> {
    unsupported()
}
pub fn process_alive(_: ProcessIdentity) -> io::Result<bool> {
    unsupported()
}

#[derive(Debug)]
pub struct Job;

impl Job {
    pub fn create(_: &str) -> io::Result<Self> {
        unsupported()
    }
    pub fn open(_: &str) -> io::Result<Option<Self>> {
        unsupported()
    }
    pub fn assign_current_process(&self) -> io::Result<()> {
        unsupported()
    }
    pub fn active_processes(&self) -> io::Result<u32> {
        unsupported()
    }
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
pub fn uninstall_entries(_: usize) -> io::Result<(Vec<UninstallEntry>, bool)> {
    unsupported()
}
pub fn local_utc_offset_seconds() -> i64 {
    0
}
pub fn show_error(title: &str, message: &str) {
    eprintln!("{title}: {message}");
}
