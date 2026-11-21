//! The token user's inbox: list, mark one read, mark all read.
use rmcp::ErrorData;
use rmcp::model::{CallToolRequestParams, Tool};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{EmptyArgs, arguments, definition, parse_id, write_definition};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{Authorized, TokenApiError};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListNotificationsArgs {
    /// `true`: only unread notifications.
    #[serde(default)]
    unread: bool,
    /// The next_cursor of the previous page.
    cursor: Option<String>,
    /// Notifications per page, from 1 to 100. Default: 50. A page can hold fewer; follow
    /// next_cursor.
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NotificationArgs {
    /// A notification id, from list_notifications.
    notification_id: String,
}

pub(super) fn scope(name: &str) -> Option<ApiTokenScope> {
    match name {
        "list_notifications" => Some(ApiTokenScope::TasksRead),
        "mark_notification_read" | "mark_all_notifications_read" => Some(ApiTokenScope::TasksWrite),
        _ => None,
    }
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        definition::<ListNotificationsArgs>(
            "list_notifications",
            "The token user's notifications (inbox), newest first: assignments, mentions in tasks, docs and chat, finished imports, GitHub pull request notices. Only those that this connection may read: task notifications of approved projects, doc mentions with docs access, chat mentions with chat access. read_at is null while unread. Not for service-account tokens.",
        ),
        write_definition::<NotificationArgs>(
            "mark_notification_read",
            "Mark one notification from list_notifications as read. Returns it.",
            false,
        ),
        write_definition::<EmptyArgs>(
            "mark_all_notifications_read",
            "Mark every unread notification that list_notifications shows as read (at most 5,000 per call). Notifications this connection cannot read stay unread. Returns how many changed.",
            false,
        ),
    ]
}

/// Runs a tool of this module; `None` for another tool's name.
pub(super) async fn call(
    api: &Authorized<'_>,
    request: &CallToolRequestParams,
) -> Option<Result<Result<Value, TokenApiError>, ErrorData>> {
    let result = match request.name.as_ref() {
        "list_notifications" => {
            let run = async {
                let a: ListNotificationsArgs = arguments(request)?;
                Ok(api
                    .notifications(a.unread, a.cursor.as_deref(), a.limit)
                    .await
                    .map(|page| json!(page)))
            };
            run.await
        }
        "mark_notification_read" => {
            let run = async {
                let a: NotificationArgs = arguments(request)?;
                Ok(api
                    .mark_notification_read(parse_id(&a.notification_id)?)
                    .await
                    .map(|notification| json!(notification)))
            };
            run.await
        }
        "mark_all_notifications_read" => {
            let run = async {
                let _: EmptyArgs = arguments(request)?;
                Ok(api
                    .mark_notifications_read()
                    .await
                    .map(|updated| json!({ "updated": updated })))
            };
            run.await
        }
        _ => return None,
    };
    Some(result)
}
