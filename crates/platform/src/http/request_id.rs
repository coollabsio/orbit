use std::fmt;

use crate::Id;

const MAX_REQUEST_ID_BYTES: usize = 128;

/// Correlates a request across HTTP handlers, logs, jobs, and error responses.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RequestId(String);

impl RequestId {
    #[must_use]
    pub fn new() -> Self {
        Self(Id::new_v7().to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn from_trusted_header(value: &str) -> Option<Self> {
        let valid = !value.is_empty()
            && value.len() <= MAX_REQUEST_ID_BYTES
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._:".contains(&byte));
        valid.then(|| Self(value.to_owned()))
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
