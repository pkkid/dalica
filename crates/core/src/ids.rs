//! Id newtypes (`control-protocol.md` §5.4).
//!
//! - [`PhotoId`] and [`FolderId`] are integers the library index assigns; stable for the life of
//!   the index, renumbered by a rebuild. Paths are the durable identity.
//! - [`AlbumId`] and [`PersonId`] are short lowercase hex strings stored in sidecars, stable across
//!   rebuilds: a one-letter prefix (`a` for albums, `p` for people) and 4 to 16 hex digits, e.g.
//!   `a91c0e`, `p7f3a2c` (`data-model.md` §3). Anything else is refused with [`Error::InvalidId`].

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, excerpt};

macro_rules! int_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = Error;

            fn from_str(s: &str) -> Result<Self, Error> {
                // Plain decimal digits only: no sign, no whitespace, no leading `+`.
                if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(Error::InvalidId { kind: stringify!($name), value: excerpt(s) });
                }
                s.parse().map(Self).map_err(|_| Error::InvalidId { kind: stringify!($name), value: excerpt(s) })
            }
        }
    };
}

int_id!(
    /// A photo in the library index.
    PhotoId
);
int_id!(
    /// A folder in the library index.
    FolderId
);

/// Fewest and most hex digits after the prefix of an [`AlbumId`] or [`PersonId`].
pub const HEX_DIGITS: std::ops::RangeInclusive<usize> = 4..=16;

macro_rules! hex_id {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The one-letter prefix every id of this kind starts with.
            pub const PREFIX: char = $prefix;

            /// Checks the shape: the prefix, then 4 to 16 lowercase hex digits.
            pub fn parse(s: &str) -> Result<Self, Error> {
                let ok = s.strip_prefix($prefix).is_some_and(|hex| {
                    HEX_DIGITS.contains(&hex.len()) && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                });
                if ok { Ok(Self(s.to_string())) } else { Err(Error::InvalidId { kind: stringify!($name), value: excerpt(s) }) }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = Error;

            fn from_str(s: &str) -> Result<Self, Error> {
                Self::parse(s)
            }
        }

        impl TryFrom<String> for $name {
            type Error = Error;

            fn try_from(s: String) -> Result<Self, Error> {
                Self::parse(&s)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }
    };
}

hex_id!(
    /// An album, as stored in sidecars and `albums.json`.
    AlbumId,
    'a'
);
hex_id!(
    /// A person (face group), as stored in sidecars.
    PersonId,
    'p'
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_ids_round_trip_as_plain_numbers() {
        let id = PhotoId::new(42);
        assert_eq!(serde_json::to_string(&id).unwrap(), "42");
        assert_eq!(serde_json::from_str::<PhotoId>("42").unwrap(), id);
        assert_eq!(id.to_string(), "42");
        assert_eq!("42".parse::<PhotoId>().unwrap(), id);
        assert_eq!(FolderId::new(u64::MAX).to_string().parse::<FolderId>().unwrap().get(), u64::MAX);
    }

    #[test]
    fn integer_ids_refuse_anything_but_digits() {
        for bad in ["", "-1", "+1", " 1", "1 ", "1.0", "0x10", "18446744073709551616", "١"] {
            assert!(bad.parse::<PhotoId>().is_err(), "{bad:?}");
        }
        assert!(serde_json::from_str::<FolderId>("-1").is_err());
        assert!(serde_json::from_str::<FolderId>("\"1\"").is_err());
    }

    #[test]
    fn hex_ids_accept_the_documented_examples() {
        assert_eq!(AlbumId::parse("a91c0e").unwrap().as_str(), "a91c0e");
        assert_eq!(PersonId::parse("p7f3a2c").unwrap().to_string(), "p7f3a2c");
        let json = serde_json::to_string(&AlbumId::parse("a77d02").unwrap()).unwrap();
        assert_eq!(json, "\"a77d02\"");
        assert_eq!(serde_json::from_str::<AlbumId>(&json).unwrap().as_str(), "a77d02");
    }

    #[test]
    fn hex_ids_refuse_other_shapes() {
        for bad in ["", "a", "a123", "A91c0e", "a91C0E", "p7f3a2c", "a12345678901234567", "a91c0g", "a91c0e ", "../a91c0e", "a91c\u{0}0e"] {
            assert!(AlbumId::parse(bad).is_err(), "{bad:?}");
        }
        assert!(PersonId::parse("a91c0e").is_err());
        assert!(serde_json::from_str::<PersonId>("\"x\"").is_err());
        assert!(serde_json::from_str::<PersonId>("7").is_err());
    }

    #[test]
    fn hex_id_lengths_are_inclusive() {
        assert!(AlbumId::parse("a1234").is_ok());
        assert!(AlbumId::parse(&format!("a{}", "f".repeat(16))).is_ok());
        assert!(AlbumId::parse(&format!("a{}", "f".repeat(17))).is_err());
    }
}
