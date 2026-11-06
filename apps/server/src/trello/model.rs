//! Tolerant serde models of the Trello REST objects the importer reads
//! (<https://developer.atlassian.com/cloud/trello/rest/>), with the field lists that fetch them.
//! Every optional field defaults, so an object with fields the token cannot see still decodes.
//! Cards are stored in `trello_import_items.data_json` in this shape, with their checklists and
//! comments attached.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A position or number Trello may send as a JSON number or a numeric string.
fn lenient_f64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Number(number) => number.as_f64().unwrap_or_default(),
        Value::String(text) => text.trim().parse().unwrap_or_default(),
        _ => 0.0,
    })
}

/// `null` decodes as the default (Trello sends `null` for empty lists and flags).
fn null_default<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloMember {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub full_name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

impl TrelloMember {
    /// The name Trello shows: the full name, else the username.
    #[must_use]
    pub fn display(&self) -> Option<String> {
        self.full_name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| self.username.clone().filter(|name| !name.trim().is_empty()))
            .map(|name| name.trim().to_owned())
    }
}

pub const MEMBER_FIELDS: &str = "fullName,username";

/// A Trello Workspace (an organization in the API).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloOrganization {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

pub const ORGANIZATION_FIELDS: &str = "displayName,name";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloBoard {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub closed: bool,
    #[serde(default)]
    pub id_organization: Option<String>,
}

pub const BOARD_FIELDS: &str = "name,closed,idOrganization";

/// A list. With `cards=open` the scan reads its open cards (ids only).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TrelloList {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub closed: bool,
    #[serde(default, deserialize_with = "lenient_f64")]
    pub pos: f64,
    #[serde(default, deserialize_with = "null_default")]
    pub cards: Vec<IdRef>,
}

pub const LIST_FIELDS: &str = "name,closed,pos";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct IdRef {
    #[serde(default)]
    pub id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TrelloLabel {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// `green`, `red_dark`, … or `null` for a label without color.
    #[serde(default)]
    pub color: Option<String>,
}

pub const LABEL_FIELDS: &str = "name,color";

/// A Power-Up custom field definition.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CustomField {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// `text`, `number`, `date`, `checkbox` or `list`.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub options: Vec<CustomFieldOption>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CustomFieldOption {
    pub id: String,
    #[serde(default)]
    pub value: Option<Value>,
}

/// A card's value of a custom field: `value` holds `text`, `number`, `date` or `checked`;
/// a list field sets `id_value` (an option id) instead.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomFieldItem {
    #[serde(default)]
    pub id_custom_field: String,
    #[serde(default)]
    pub id_value: Option<String>,
    #[serde(default)]
    pub value: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloAttachment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// A file uploaded to Trello (downloaded with the key and token); otherwise a link.
    #[serde(default, deserialize_with = "null_default")]
    pub is_upload: bool,
    #[serde(default)]
    pub bytes: Option<u64>,
}

pub const ATTACHMENT_FIELDS: &str = "name,url,isUpload,bytes";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CheckItem {
    #[serde(default)]
    pub name: Option<String>,
    /// `complete` or `incomplete`.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default, deserialize_with = "lenient_f64")]
    pub pos: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloChecklist {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub id_card: String,
    #[serde(default, deserialize_with = "lenient_f64")]
    pub pos: f64,
    #[serde(default, deserialize_with = "null_default")]
    pub check_items: Vec<CheckItem>,
}

pub const CHECKLIST_FIELDS: &str = "name,idCard,pos";
pub const CHECK_ITEM_FIELDS: &str = "name,state,pos";

/// A `commentCard` action, as the importer keeps it on its card.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TrelloComment {
    #[serde(default)]
    pub id: String,
    /// Markdown.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
}

/// A `commentCard` action as the API sends it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentAction {
    pub id: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub data: CommentData,
    #[serde(default)]
    pub member_creator: Option<TrelloMember>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CommentData {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub card: Option<IdRef>,
}

pub const ACTION_FIELDS: &str = "data,date";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloCard {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Markdown.
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub closed: bool,
    #[serde(default)]
    pub id_list: String,
    #[serde(default, deserialize_with = "null_default")]
    pub id_labels: Vec<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub id_members: Vec<String>,
    /// RFC 3339 instants.
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub due_complete: bool,
    #[serde(default, deserialize_with = "lenient_f64")]
    pub pos: f64,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub short_url: Option<String>,
    #[serde(default)]
    pub date_last_activity: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub attachments: Vec<TrelloAttachment>,
    #[serde(default, deserialize_with = "null_default")]
    pub custom_field_items: Vec<CustomFieldItem>,
    /// Filled by the importer from the board's checklists.
    #[serde(default)]
    pub checklists: Vec<TrelloChecklist>,
    /// Filled by the importer from the board's comments, oldest first.
    #[serde(default)]
    pub comments: Vec<TrelloComment>,
}

pub const CARD_FIELDS: &str = "name,desc,closed,idList,idLabels,idMembers,start,due,dueComplete,\
pos,url,shortUrl,dateLastActivity";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_sparse_cards_and_lenient_numbers() {
        let card: TrelloCard = serde_json::from_str(
            r#"{"id": "c", "pos": "16384.5", "idLabels": null, "attachments": [{"id": "a", "isUpload": true}], "unknown": 1}"#,
        )
        .unwrap();
        assert_eq!(card.id, "c");
        assert!((card.pos - 16_384.5).abs() < f64::EPSILON);
        assert!(card.id_labels.is_empty());
        assert!(card.attachments[0].is_upload);
        let member = TrelloMember {
            username: Some("ada".into()),
            ..TrelloMember::default()
        };
        assert_eq!(member.display().as_deref(), Some("ada"));
        assert_eq!(TrelloMember::default().display(), None);
    }
}
