use std::fmt;

use serde::Serialize;
use uuid::Uuid;

use crate::error::DomainError;

const PREFIX: &str = "DRN-";
const SUFFIX_LEN: usize = 12;

/// Customer-facing identifier, e.g. `DRN-3F9A1C07B2E4`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct TrackingNumber(String);

impl TrackingNumber {
    pub fn generate() -> Self {
        let random = Uuid::new_v4().simple().to_string();
        let suffix = random[..SUFFIX_LEN].to_ascii_uppercase();
        Self(format!("{PREFIX}{suffix}"))
    }

    /// Parses user input; the lookup is case-insensitive.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let normalized = value.trim().to_ascii_uppercase();
        let well_formed = normalized.strip_prefix(PREFIX).is_some_and(|suffix| {
            suffix.len() == SUFFIX_LEN && suffix.chars().all(|c| c.is_ascii_hexdigit())
        });
        if !well_formed {
            return Err(DomainError::InvalidTrackingNumber);
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TrackingNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
