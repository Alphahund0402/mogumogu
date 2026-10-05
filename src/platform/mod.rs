//! Operating-system boundary. All `unsafe` FFI of the crate lives in the
//! `windows` submodule behind a small safe API; nothing else touches raw
//! handles. Other platforms get stubs that report `Unsupported`.
use std::fmt;
use std::str::FromStr;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
mod fallback;
#[cfg(not(windows))]
pub use self::fallback::*;

/// Volume serial plus 128-bit file id: the identity of a filesystem object,
/// independent of the path used to reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    pub volume: u64,
    pub id: [u8; 16],
}

impl fmt::Display for FileIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}:", self.volume)?;
        self.id.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl FromStr for FileIdentity {
    type Err = crate::Error;

    fn from_str(text: &str) -> crate::Result<Self> {
        let invalid = || crate::Error::invalid("Gespeicherte Dateiidentität ist ungültig.");
        let (volume, id) = text.split_once(':').ok_or_else(invalid)?;
        let volume = u64::from_str_radix(volume, 16).map_err(|_| invalid())?;
        if id.len() != 32 {
            return Err(invalid());
        }
        let mut bytes = [0_u8; 16];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&id[i * 2..i * 2 + 2], 16).map_err(|_| invalid())?;
        }
        Ok(Self { volume, id: bytes })
    }
}

/// Attribute bits used by the safety checks.
pub mod attributes {
    pub const READONLY: u32 = 0x1;
    pub const DIRECTORY: u32 = 0x10;
    pub const REPARSE_POINT: u32 = 0x400;
    pub const OFFLINE: u32 = 0x1000;
    pub const RECALL_ON_OPEN: u32 = 0x4_0000;
    pub const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

    /// Cloud or offline placeholder: reading would hydrate remote content.
    pub fn is_placeholder(value: u32) -> bool {
        value & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
    }
}

/// One directory entry as reported by the filesystem, without following it.
#[derive(Clone, Debug)]
pub struct DirEntryInfo {
    pub name: std::ffi::OsString,
    pub attributes: u32,
    pub size: u64,
    /// Last write time, unix seconds.
    pub modified: i64,
}

impl DirEntryInfo {
    pub fn is_dir(&self) -> bool {
        self.attributes & attributes::DIRECTORY != 0
    }
    pub fn is_reparse(&self) -> bool {
        self.attributes & attributes::REPARSE_POINT != 0
    }
    pub fn is_placeholder(&self) -> bool {
        attributes::is_placeholder(self.attributes)
    }
    /// UTF-8 name; entries with ill-formed names are reported, not guessed.
    pub fn name_str(&self) -> Option<&str> {
        self.name.to_str()
    }
}

/// Metadata of an opened object, read from its handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandleStat {
    pub identity: FileIdentity,
    pub attributes: u32,
    pub size: u64,
    pub modified: i64,
}

/// An entry of the Windows "installed programs" list (ARP). Presence in this
/// list says nothing about which manager installed or manages the program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UninstallEntry {
    pub hive: &'static str,
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
}

/// FILETIME ticks (100 ns since 1601) to unix seconds.
pub fn filetime_to_unix(ticks: i64) -> i64 {
    const EPOCH_DIFF: i64 = 116_444_736_000_000_000;
    (ticks - EPOCH_DIFF).div_euclid(10_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_roundtrips_as_text() {
        let id = FileIdentity { volume: 0xabc, id: [7; 16] };
        assert_eq!(id.to_string().parse::<FileIdentity>().unwrap(), id);
        assert!("nonsense".parse::<FileIdentity>().is_err());
    }

    #[test]
    fn filetime_epoch() {
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0);
    }
}
