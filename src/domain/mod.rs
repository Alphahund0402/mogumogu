//! Typed domain records and state machines. No I/O lives here.
//!
//! Ownership, activity, availability, protection and cleanup eligibility are
//! independent properties (Implementierungsplan §6); every type below keeps
//! them in separate fields instead of deriving one from another.

/// Generates a closed string enum with SQLite and serde mappings, so stored
/// states are validated on every read instead of trusted as free text.
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis enum $name { $($(#[$vmeta])* $variant),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
            pub fn parse(text: &str) -> Option<Self> {
                match text { $($text => Some(Self::$variant),)+ _ => None }
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = String::deserialize(d)?;
                Self::parse(&text)
                    .ok_or_else(|| serde::de::Error::custom(format!("unbekannter Wert: {text}")))
            }
        }
        impl rusqlite::types::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }
        impl rusqlite::types::FromSql for $name {
            fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                let text = value.as_str()?;
                Self::parse(text).ok_or_else(|| {
                    rusqlite::types::FromSqlError::Other(
                        format!("{}: unbekannter Wert {text}", stringify!($name)).into(),
                    )
                })
            }
        }
    };
}

mod cleanup;
mod inventory;
mod project;
mod review;
mod scope;
mod session;
mod settings;
mod snapshot;
mod state;

pub use cleanup::*;
pub use inventory::*;
pub use project::*;
pub use review::*;
pub use scope::*;
pub use session::*;
pub use settings::*;
pub use snapshot::*;
pub use state::*;
