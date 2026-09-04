use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use uuid::{Uuid, Variant};

/// A time-ordered persistent identifier.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(Uuid);

impl Id {
    #[must_use]
    pub fn new_v7() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

#[derive(Debug, Error)]
pub enum ParseIdError {
    #[error("invalid UUID: {0}")]
    InvalidUuid(#[from] uuid::Error),
    #[error("ID must use the canonical lowercase UUID representation")]
    NotCanonical,
    #[error("ID must be a UUIDv7")]
    NotVersion7,
}

impl FromStr for Id {
    type Err = ParseIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(value)?;
        if uuid.to_string() != value {
            return Err(ParseIdError::NotCanonical);
        }
        if uuid.get_version_num() != 7 || uuid.get_variant() != Variant::RFC4122 {
            return Err(ParseIdError::NotVersion7);
        }
        Ok(Self(uuid))
    }
}

impl Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}
