//! @mentions in page bodies (BlockNote `mention` inline content `{ userId, name }`) and their
//! `page_mentioned` inbox notifications.
//!
//! Every content write compares the mentioned members before and after (the collaborative
//! projection and REST edits). A member gets a notification when the write adds the first mention
//! of them to the page (moving or re-projecting a mention adds nothing), they can see the page
//! (a teamspace page: any active member; a private page: only its owner), they are not the
//! editor, and they got no page mention notification for this page in the last 10 minutes.

use std::collections::BTreeMap;

use orbit_platform::{Id, TimestampMillis};
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};

use crate::audit::{self, AuditOutcome};
use crate::collab::sanitize::is_uuid;

/// A recipient gets at most one page mention notification per page in this window.
pub const DEDUPE_WINDOW_MILLIS: i64 = 10 * 60 * 1_000;

/// Blocks ids are BlockNote's (UUIDs or short strings); longer ones are not kept as anchors.
const MAX_BLOCK_ID_CHARS: usize = 64;

/// The members a document mentions, each with the id of the first block that mentions them.
#[must_use]
pub fn mentioned_users(content: &[Value]) -> BTreeMap<Id, Option<String>> {
    let mut found = BTreeMap::new();
    for block in content {
        walk_block(block, &mut found);
    }
    found
}

fn walk_block(block: &Value, found: &mut BTreeMap<Id, Option<String>>) {
    let block_id = block
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.chars().count() <= MAX_BLOCK_ID_CHARS);
    if let Some(content) = block.get("content") {
        walk_inline(content, block_id, found);
    }
    if let Some(Value::Array(children)) = block.get("children") {
        for child in children {
            walk_block(child, found);
        }
    }
}

/// Inline content anywhere below a block's `content` (paragraph runs, table cells).
fn walk_inline(value: &Value, block_id: Option<&str>, found: &mut BTreeMap<Id, Option<String>>) {
    match value {
        Value::Array(items) => {
            for item in items {
                walk_inline(item, block_id, found);
            }
        }
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("mention") => {
            if let Some(user_id) = map
                .get("props")
                .and_then(|props| props.get("userId"))
                .and_then(Value::as_str)
                .filter(|id| is_uuid(id, false))
                .and_then(|id| id.to_ascii_lowercase().parse::<Id>().ok())
            {
                found
                    .entry(user_id)
                    .or_insert_with(|| block_id.map(str::to_owned));
            }
        }
        Value::Object(map) => {
            for child in map.values() {
                walk_inline(child, block_id, found);
            }
        }
        _ => {}
    }
}

/// Notifies the members `after` mentions and `before` did not (see the module docs); returns
/// the recipients. `editor` is who made the change (no editor, no notification).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn notify_new_mentions(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: Id,
    before: &[Value],
    after: &[Value],
    editor: Option<Id>,
    request_id: &str,
    now: TimestampMillis,
) -> Result<Vec<Id>, sqlx::Error> {
    let Some(editor) = editor else {
        return Ok(Vec::new());
    };
    let previous = mentioned_users(before);
    let added: Vec<(Id, Option<String>)> = mentioned_users(after)
        .into_iter()
        .filter(|(user_id, _)| *user_id != editor && !previous.contains_key(user_id))
        .collect();
    if added.is_empty() {
        return Ok(Vec::new());
    }
    let Some(page) =
        sqlx::query("SELECT workspace_id, owner_id FROM pages WHERE id = ? AND deleted_at IS NULL")
            .bind(page_id.to_string())
            .fetch_optional(&mut **tx)
            .await?
    else {
        return Ok(Vec::new());
    };
    let workspace_id: String = page.get("workspace_id");
    let owner_id: Option<String> = page.get("owner_id");
    let mut notified = Vec::new();
    for (recipient, block_id) in added {
        let recipient_text = recipient.to_string();
        // A private page is its owner's alone.
        if owner_id
            .as_deref()
            .is_some_and(|owner| owner != recipient_text)
        {
            continue;
        }
        let allowed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM memberships JOIN users ON users.id = memberships.user_id \
             WHERE memberships.workspace_id = ? AND memberships.user_id = ? \
             AND users.suspended_at IS NULL) \
             AND NOT EXISTS(SELECT 1 FROM notifications WHERE kind = 'page_mentioned' \
             AND page_id = ? AND recipient_user_id = ? AND created_at > ?)",
        )
        .bind(&workspace_id)
        .bind(&recipient_text)
        .bind(page_id.to_string())
        .bind(&recipient_text)
        .bind(now.as_millis().saturating_sub(DEDUPE_WINDOW_MILLIS))
        .fetch_one(&mut **tx)
        .await?;
        if !allowed {
            continue;
        }
        let id = Id::new_v7();
        sqlx::query(
            "INSERT OR IGNORE INTO notifications \
             (id, workspace_id, recipient_user_id, actor_user_id, kind, page_id, page_block_id, \
              dedupe_key, created_at) \
             VALUES (?, ?, ?, ?, 'page_mentioned', ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&workspace_id)
        .bind(&recipient_text)
        .bind(editor.to_string())
        .bind(page_id.to_string())
        .bind(block_id)
        .bind(format!(
            "{workspace_id}:{recipient}:page_mentioned:{page_id}:{id}"
        ))
        .bind(now.as_millis())
        .execute(&mut **tx)
        .await?;
        notified.push(recipient);
    }
    if !notified.is_empty() {
        // Also the realtime signal that refreshes the recipients' inboxes.
        let workspace: Id = workspace_id
            .parse()
            .map_err(|_| sqlx::Error::Decode("invalid workspace id".into()))?;
        audit::record(
            tx,
            workspace,
            Some(editor),
            "page.mentioned",
            AuditOutcome::Success,
            "page",
            Some(page_id),
            request_id,
            json!({ "recipients": notified.len() }),
            now,
        )
        .await?;
    }
    Ok(notified)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ANN: &str = "0199a0b0-0000-7000-8000-0000000000a1";
    const BOB: &str = "0199a0b0-0000-7000-8000-0000000000b2";

    fn mention(user: &str) -> Value {
        json!({"type": "mention", "props": {"userId": user, "name": "x"}})
    }

    #[test]
    fn mentions_are_collected_with_their_first_block() {
        let content = [
            json!({"id": "p1", "type": "paragraph", "content": [mention(ANN), mention("nope")],
                   "children": [{"id": "c1", "type": "paragraph", "content": [mention(BOB)]}]}),
            json!({"id": "t1", "type": "table", "content": {"type": "tableContent", "rows": [
                {"cells": [[mention(&ANN.to_uppercase())], {"type": "tableCell", "content": [mention(BOB)]}]}
            ]}}),
            json!({"id": "t2", "type": "paragraph", "props": {"type": "mention"}, "content": "plain"}),
        ];
        let found = mentioned_users(&content);
        assert_eq!(found.len(), 2);
        assert_eq!(found[&ANN.parse::<Id>().unwrap()].as_deref(), Some("p1"));
        assert_eq!(found[&BOB.parse::<Id>().unwrap()].as_deref(), Some("c1"));
        assert!(mentioned_users(&[json!({"type": "paragraph", "content": []})]).is_empty());
    }
}
