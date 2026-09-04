use std::collections::BTreeMap;

use serde::Serialize;
use utoipa::ToSchema;

const VALIDATION_TYPE: &str = "https://docs.orbit.dev/problems/validation-failed";
const INTERNAL_TYPE: &str = "https://docs.orbit.dev/problems/internal-error";
const CONFLICT_TYPE: &str = "https://docs.orbit.dev/problems/conflict";

/// An RFC 9457 response body with Orbit's stable machine-readable extensions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct Problem {
    #[serde(rename = "type")]
    pub type_uri: String,
    pub title: String,
    pub status: u16,
    pub code: String,
    pub detail: String,
    pub instance: String,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<BTreeMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<ConflictMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct ConflictMetadata {
    pub current_version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh: Option<String>,
}

impl Problem {
    pub fn validation<K, V, I>(
        request_id: impl Into<String>,
        instance: impl Into<String>,
        errors: I,
    ) -> Self
    where
        K: Into<String>,
        V: Into<String>,
        I: IntoIterator<Item = (K, V)>,
    {
        let mut fields = BTreeMap::<String, Vec<String>>::new();
        for (field, message) in errors {
            fields.entry(field.into()).or_default().push(message.into());
        }
        Self {
            type_uri: VALIDATION_TYPE.to_owned(),
            title: "Validation failed".to_owned(),
            status: 422,
            code: "validation_failed".to_owned(),
            detail: "One or more fields are invalid.".to_owned(),
            instance: instance.into(),
            request_id: request_id.into(),
            errors: Some(fields),
            conflict: None,
        }
    }

    /// Builds a generic internal error without accepting private error text.
    #[must_use]
    pub fn internal(request_id: impl Into<String>, instance: impl Into<String>) -> Self {
        Self {
            type_uri: INTERNAL_TYPE.to_owned(),
            title: "Internal server error".to_owned(),
            status: 500,
            code: "internal_error".to_owned(),
            detail: "An unexpected error occurred. Use the request ID when contacting support."
                .to_owned(),
            instance: instance.into(),
            request_id: request_id.into(),
            errors: None,
            conflict: None,
        }
    }

    #[must_use]
    pub fn conflict(
        request_id: impl Into<String>,
        instance: impl Into<String>,
        detail: impl Into<String>,
        current_version: i64,
        refresh: Option<String>,
    ) -> Self {
        Self {
            type_uri: CONFLICT_TYPE.to_owned(),
            title: "Conflict".to_owned(),
            status: 409,
            code: "conflict".to_owned(),
            detail: detail.into(),
            instance: instance.into(),
            request_id: request_id.into(),
            errors: None,
            conflict: Some(ConflictMetadata {
                current_version,
                refresh,
            }),
        }
    }
}
