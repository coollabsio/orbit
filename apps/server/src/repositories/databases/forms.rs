//! Form views (spec P3 item 16, D-P3-4): members who can read a database fill a form of chosen
//! properties, and each answer becomes a new row that they created. There are no public forms.
//! The layout is the view's config (`ViewConfig::form`), so it follows the view edit rules; a
//! submission is a row write, so it also works on a locked database.

use std::collections::{HashMap, HashSet};

use orbit_platform::{Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Any, Transaction};
use utoipa::ToSchema;

use super::{
    DatabaseError, DatabaseRepository, DatabaseRow, MAX_NAME_CHARS, MAX_PROPERTIES, NewRow,
    Property, PropertyKind, PropertyType, RowPlacement, ViewConfig, ViewType, Views, is_computed,
    open, parse_value, properties_in_tx, record_with, views_in_tx,
};

/// Submissions of one member in one workspace per hour (spec section 19).
pub const MAX_SUBMISSIONS_PER_HOUR: i64 = 30;
const HOUR_MS: i64 = 60 * 60 * 1_000;
const MAX_FORM_TEXT_CHARS: usize = 2_000;
const MAX_TITLE_ANSWER_CHARS: usize = 500;
/// The audit action of a submission; the rate limit counts these.
const SUBMITTED: &str = "database.form_submitted";

/// Who may submit: members who can read the database, or nobody.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseFormAccess)]
pub enum FormAccess {
    #[default]
    Members,
    /// The form shows that it takes no answers; a submission is refused (409).
    Closed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseFormQuestion)]
pub struct FormQuestion {
    /// The title or a property that stores values (not a computed one).
    #[schema(value_type = String)]
    pub property_id: Id,
    /// Shown instead of the property name. At most 200 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Help text under the question. At most 2,000 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// A form view's layout. A form view without one starts with the title as its one required
/// question.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseFormConfig)]
pub struct FormConfig {
    /// In order. Questions of deleted or computed properties, and repeats, are dropped.
    #[serde(default)]
    pub questions: Vec<FormQuestion>,
    /// The heading; absent: the database's title. At most 200 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// At most 2,000 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The submit button; absent: "Submit". At most 200 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit_label: Option<String>,
    /// The thank-you text after a submission. At most 2,000 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<String>,
    #[serde(default)]
    pub access: FormAccess,
}

/// A property a form can ask for: the title, or one whose values members type. Not computed,
/// related or generated properties, buttons, or files (a files value names files of the row page,
/// which does not exist before the submission).
fn answerable(kind: &PropertyKind) -> bool {
    let property_type = kind.property_type();
    property_type == PropertyType::Title
        || !(property_type.computed()
            || matches!(property_type, PropertyType::UniqueId | PropertyType::Files)
            || is_computed(kind))
}

fn too_long(text: Option<&String>, max: usize) -> bool {
    text.is_some_and(|text| text.chars().count() > max)
}

/// The form part of `ViewConfig::validate`: limits, with the field path.
pub(super) fn check(config: &ViewConfig) -> Result<(), String> {
    let Some(form) = &config.form else {
        return Ok(());
    };
    if form.questions.len() > MAX_PROPERTIES {
        return Err("config.form.questions".to_owned());
    }
    for (index, question) in form.questions.iter().enumerate() {
        if too_long(question.label.as_ref(), MAX_NAME_CHARS) {
            return Err(format!("config.form.questions[{index}].label"));
        }
        if too_long(question.description.as_ref(), MAX_FORM_TEXT_CHARS) {
            return Err(format!("config.form.questions[{index}].description"));
        }
    }
    let fields = [
        ("title", &form.title, MAX_NAME_CHARS),
        ("description", &form.description, MAX_FORM_TEXT_CHARS),
        ("submit_label", &form.submit_label, MAX_NAME_CHARS),
        ("confirmation", &form.confirmation, MAX_FORM_TEXT_CHARS),
    ];
    for (name, text, max) in fields {
        if too_long(text.as_ref(), max) {
            return Err(format!("config.form.{name}"));
        }
    }
    Ok(())
}

/// The form part of `ViewConfig::repair`: only form views keep a form, and a form keeps one
/// question for each answerable property it names.
pub(super) fn repair(config: &mut ViewConfig, view_type: ViewType, properties: &[Property]) {
    if view_type != ViewType::Form {
        config.form = None;
        return;
    }
    let form = config.form.get_or_insert_with(|| FormConfig {
        questions: properties
            .iter()
            .filter(|property| property.kind == PropertyKind::Title)
            .map(|property| FormQuestion {
                property_id: property.id,
                label: None,
                description: None,
                required: true,
            })
            .collect(),
        ..FormConfig::default()
    });
    let kinds: HashMap<Id, &PropertyKind> = properties
        .iter()
        .map(|property| (property.id, &property.kind))
        .collect();
    let mut seen = HashSet::new();
    form.questions.retain(|question| {
        kinds
            .get(&question.property_id)
            .is_some_and(|kind| answerable(kind))
            && seen.insert(question.property_id)
    });
}

pub(super) fn remap(config: &mut ViewConfig, properties: &HashMap<Id, Id>) {
    if let Some(form) = &mut config.form {
        for question in &mut form.questions {
            if let Some(copy) = properties.get(&question.property_id) {
                question.property_id = *copy;
            }
        }
    }
}

/// One member's answers to a form view, by property id.
pub struct FormSubmission {
    pub view_id: Id,
    pub answers: Vec<(Id, Value)>,
}

impl DatabaseRepository {
    /// A new row (last in manual order) from a form view's answers, created by `actor_id`, in one
    /// transaction with the form's checks.
    pub async fn submit_form(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        submission: &FormSubmission,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseRow, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        open(&mut tx, workspace_id, database_id, actor_id).await?;
        let mut input = NewRow {
            title: String::new(),
            icon: None,
            values: Vec::new(),
            placement: RowPlacement::Last,
        };
        admit(
            &mut tx,
            workspace_id,
            database_id,
            actor_id,
            submission,
            &mut input,
            now,
        )
        .await?;
        let row = Self::insert_row_in_tx(
            &self.database,
            &mut tx,
            Id::new_v7(),
            workspace_id,
            database_id,
            actor_id,
            input,
            None,
            request_id,
            now,
        )
        .await?;
        record_with(
            &mut tx,
            workspace_id,
            actor_id,
            SUBMITTED,
            database_id,
            json!({ "row_id": row.id.to_string(), "view_id": submission.view_id.to_string() }),
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }
}

/// Inside the submission's write transaction: refuses a closed form and a member over the hourly limit,
/// then checks the answers against the questions and puts them into `input`. Answers to
/// properties the form does not ask for and empty required answers are 422 `answers.<id>`.
async fn admit(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    actor_id: Id,
    submission: &FormSubmission,
    input: &mut NewRow,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    let properties = properties_in_tx(tx, database_id).await?;
    let form = views_in_tx(tx, database_id, Views::All, &properties)
        .await?
        .into_iter()
        .find(|view| view.id == submission.view_id && view.view_type == ViewType::Form)
        .and_then(|view| view.config.form)
        .ok_or(DatabaseError::ViewNotFound)?;
    if form.access == FormAccess::Closed {
        return Err(DatabaseError::FormClosed);
    }
    let recent: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS BIGINT) FROM audit_events WHERE workspace_id = $1 \
         AND occurred_at > $2 AND actor_id = $3 AND action = $4",
    )
    .bind(workspace_id.to_string())
    .bind(now.as_millis() - HOUR_MS)
    .bind(actor_id.to_string())
    .bind(SUBMITTED)
    .fetch_one(&mut **tx)
    .await?;
    if recent >= MAX_SUBMISSIONS_PER_HOUR {
        return Err(DatabaseError::FormRateLimit);
    }
    let mut answers: HashMap<Id, &Value> = submission
        .answers
        .iter()
        .map(|(id, value)| (*id, value))
        .collect();
    for question in &form.questions {
        let invalid = || DatabaseError::invalid(format!("answers.{}", question.property_id));
        let Some(property) = properties.iter().find(|p| p.id == question.property_id) else {
            continue;
        };
        let raw = answers
            .remove(&question.property_id)
            .unwrap_or(&Value::Null);
        let filled = if property.kind == PropertyKind::Title {
            let title = match raw {
                Value::Null => "",
                Value::String(text) => text.trim(),
                _ => return Err(invalid()),
            };
            if title.chars().count() > MAX_TITLE_ANSWER_CHARS {
                return Err(invalid());
            }
            input.title = title.to_owned();
            !title.is_empty()
        } else {
            let value = parse_value(&property.kind, raw).map_err(|()| invalid())?;
            if value.is_some() {
                input.values.push((property.id, raw.clone()));
            }
            value.is_some()
        };
        if question.required && !filled {
            return Err(invalid());
        }
    }
    if let Some(id) = answers.keys().next() {
        return Err(DatabaseError::invalid(format!("answers.{id}")));
    }
    Ok(())
}
