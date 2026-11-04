//! Tolerant serde models of the Asana API objects the importer reads. Every field the importer
//! does not need is ignored, and every optional one defaults, so a response with fields left out
//! (compact objects, `opt_fields` the token cannot see) still decodes.

use serde::{Deserialize, Serialize};

/// A compact object with a name (team, tag, section, project, enum option, user).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Named {
    pub gid: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// `GET /users/me`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AsanaUser {
    pub gid: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AsanaWorkspace {
    pub gid: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AsanaProject {
    pub gid: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub archived: bool,
    /// Only organizations have teams.
    #[serde(default)]
    pub team: Option<Named>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CustomField {
    #[serde(default)]
    pub name: Option<String>,
    /// The value as Asana shows it, for every field type.
    #[serde(default)]
    pub display_value: Option<String>,
    #[serde(default)]
    pub enum_value: Option<Named>,
}

/// A task's place in one project.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Membership {
    #[serde(default)]
    pub project: Option<Named>,
    #[serde(default)]
    pub section: Option<Named>,
}

/// A task as `GET /projects/{gid}/tasks` and `GET /tasks/{gid}/subtasks` list it with
/// [`TASK_FIELDS`]. Stored in `asana_import_items.data_json` until the task is filled.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AsanaTask {
    pub gid: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub html_notes: Option<String>,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub due_on: Option<String>,
    #[serde(default)]
    pub due_at: Option<String>,
    #[serde(default)]
    pub start_on: Option<String>,
    #[serde(default)]
    pub start_at: Option<String>,
    #[serde(default)]
    pub permalink_url: Option<String>,
    #[serde(default)]
    pub assignee: Option<AsanaUser>,
    #[serde(default)]
    pub tags: Vec<Named>,
    #[serde(default)]
    pub custom_fields: Vec<CustomField>,
    #[serde(default)]
    pub memberships: Vec<Membership>,
    #[serde(default)]
    pub num_subtasks: u32,
    /// Attachments listed when the task was created (`None` until then).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<AsanaAttachment>>,
}

/// `opt_fields` for task lists: everything [`AsanaTask`] reads.
pub const TASK_FIELDS: &str = "name,notes,html_notes,completed,completed_at,due_on,due_at,\
start_on,start_at,permalink_url,assignee.name,assignee.email,tags.name,custom_fields.name,\
custom_fields.display_value,custom_fields.enum_value.name,memberships.project.name,\
memberships.section.name,num_subtasks";

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AsanaStory {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub resource_subtype: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub html_text: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub created_by: Option<Named>,
}

impl AsanaStory {
    /// A comment someone wrote (not a system story such as "assigned to …").
    #[must_use]
    pub fn is_comment(&self) -> bool {
        self.resource_subtype.as_deref() == Some("comment_added")
            || self.kind.as_deref() == Some("comment")
    }
}

pub const STORY_FIELDS: &str = "type,resource_subtype,text,html_text,created_at,created_by.name";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AsanaAttachment {
    pub gid: String,
    #[serde(default)]
    pub name: Option<String>,
    /// `asana` for files uploaded to Asana; `dropbox`, `gdrive`, `onedrive`, `box`, `vimeo`
    /// or `external` for links.
    #[serde(default)]
    pub resource_subtype: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub view_url: Option<String>,
    #[serde(default)]
    pub permanent_url: Option<String>,
    /// Signed and short-lived: only read from `GET /attachments/{gid}` right before the
    /// download, never stored.
    #[serde(default, skip_serializing)]
    pub download_url: Option<String>,
}

impl AsanaAttachment {
    #[must_use]
    pub fn is_hosted(&self) -> bool {
        self.resource_subtype.as_deref() == Some("asana")
    }
}

pub const ATTACHMENT_FIELDS: &str = "name,resource_subtype,size,view_url,permanent_url";

/// Asana dates are `YYYY-MM-DD`; Orbit due dates are instants. A date becomes 12:00 UTC of that
/// day, which falls on the same calendar day from UTC-11 to UTC+11.
#[must_use]
pub fn parse_date_millis(value: &str) -> Option<i64> {
    let days = crate::repositories::task_filter::parse_ymd(value)?;
    Some(days * 86_400_000 + 12 * 3_600_000)
}

/// An RFC 3339 instant (`2024-05-01T09:30:00.000Z`, or with a `±HH:MM` offset) in epoch
/// milliseconds.
#[must_use]
pub fn parse_instant_millis(value: &str) -> Option<i64> {
    let value = value.trim();
    if !value.is_ascii() || value.len() < 20 || value.as_bytes()[10] != b'T' {
        return None;
    }
    let days = crate::repositories::task_filter::parse_ymd(&value[..10])?;
    let digits = |part: &str| -> Option<i64> {
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let rest = &value[11..];
    if rest.len() < 9 || &rest[2..3] != ":" || &rest[5..6] != ":" {
        return None;
    }
    let (hour, minute, second) = (
        digits(&rest[..2])?,
        digits(&rest[3..5])?,
        digits(&rest[6..8])?,
    );
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &rest[8..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let end = fraction
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(fraction.len());
        if end == 0 {
            return None;
        }
        let padded = format!("{:0<3}", &fraction[..end.min(3)]);
        millis = digits(&padded)?;
        rest = &fraction[end..];
    }
    let offset_minutes = match rest {
        "Z" | "z" => 0,
        _ if rest.len() == 6 && &rest[3..4] == ":" => {
            let sign = match &rest[..1] {
                "+" => 1,
                "-" => -1,
                _ => return None,
            };
            sign * (digits(&rest[1..3])? * 60 + digits(&rest[4..6])?)
        }
        _ => return None,
    };
    Some(
        days * 86_400_000
            + (hour * 3_600 + minute * 60 + second - offset_minutes * 60) * 1_000
            + millis,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_asana_dates_and_instants() {
        assert_eq!(
            parse_date_millis("1970-01-02"),
            Some(86_400_000 + 43_200_000)
        );
        assert_eq!(parse_date_millis("2024-02-30"), None);
        assert_eq!(
            parse_instant_millis("1970-01-01T00:00:01.250Z"),
            Some(1_250)
        );
        assert_eq!(parse_instant_millis("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(parse_instant_millis("1970-01-01T00:00:00.5Z"), Some(500));
        assert_eq!(parse_instant_millis("1970-01-01 00:00:00Z"), None);
        assert_eq!(parse_instant_millis("1970-01-01T00:00:00"), None);
    }

    #[test]
    fn decodes_compact_tasks() {
        let task: AsanaTask = serde_json::from_str(r#"{"gid": "1", "unknown": true}"#).unwrap();
        assert_eq!(task.gid, "1");
        assert!(!task.completed);
        assert!(task.tags.is_empty());
    }
}
