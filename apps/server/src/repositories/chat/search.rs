//! Message search (FTS5, migration 0037) over the conversations the caller can read. Every word
//! must match (the last one as a prefix); case and diacritics are ignored. Newest message first,
//! paged by message id.

use orbit_platform::Id;
use serde::Serialize;
use sqlx::Row;
use utoipa::ToSchema;

use super::{
    ChatError, ChatRepository, MESSAGE_SELECT, MessageRecord, hydrate, id_list, load_access,
    load_actor, parse_id,
};
use crate::repositories::pages::{fts_query, split_marks};

const PAGE_SIZE: usize = 20;

#[derive(Clone, Debug, Default)]
pub struct SearchInput {
    pub query: String,
    /// Only this conversation.
    pub conversation_id: Option<Id>,
    /// Only messages of this member.
    pub author_id: Option<Id>,
    /// Only messages with a file.
    pub has_file: bool,
    /// The `cursor` of the page before: messages older than it.
    pub cursor: Option<Id>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SearchHitRecord {
    pub message: MessageRecord,
    #[schema(value_type = String)]
    pub conversation_id: Id,
    /// `[start, end)` offsets in `message.body` to highlight, in UTF-16 code units.
    pub ranges: Vec<[u32; 2]>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct SearchPage {
    pub items: Vec<SearchHitRecord>,
    /// Null when there is nothing older.
    #[schema(value_type = Option<String>, required = true)]
    pub cursor: Option<Id>,
}

impl ChatRepository {
    /// Without words to search for, a filter is needed: the result is then the newest
    /// messages that pass it.
    pub async fn search_messages(
        &self,
        workspace_id: Id,
        actor_id: Id,
        input: SearchInput,
    ) -> Result<SearchPage, ChatError> {
        let mut tx = self.database.pool().begin().await?;
        match input.conversation_id {
            // Also answers `NotFound` for a conversation the caller cannot read.
            Some(conversation_id) => {
                load_access(&mut tx, workspace_id, actor_id, conversation_id).await?;
            }
            None => {
                load_actor(&mut tx, workspace_id, actor_id).await?;
            }
        }
        let expression = fts_query(&input.query);
        let filtered =
            input.conversation_id.is_some() || input.author_id.is_some() || input.has_file;
        if expression.is_none() && !filtered {
            return Ok(SearchPage {
                items: Vec::new(),
                cursor: None,
            });
        }

        let mut sql = String::from(if expression.is_some() {
            "SELECT m.id, highlight(chat_search, 0, char(2), char(3)) AS marked FROM chat_search \
             JOIN chat_messages m ON m.row_id = chat_search.rowid"
        } else {
            "SELECT m.id, m.body AS marked FROM chat_messages m"
        });
        sql.push_str(
            " JOIN chat_conversations c ON c.id = m.conversation_id \
             WHERE c.workspace_id = ? AND m.kind = 'message' AND m.deleted_at IS NULL \
             AND (c.kind = 'public' OR EXISTS (SELECT 1 FROM chat_members \
                  WHERE chat_members.conversation_id = c.id AND chat_members.user_id = ?))",
        );
        for (present, condition) in [
            (expression.is_some(), " AND chat_search MATCH ?"),
            (
                input.conversation_id.is_some(),
                " AND m.conversation_id = ?",
            ),
            (input.author_id.is_some(), " AND m.author_id = ?"),
            (
                input.has_file,
                " AND EXISTS (SELECT 1 FROM chat_message_files WHERE message_id = m.id)",
            ),
            (input.cursor.is_some(), " AND m.id < ?"),
        ] {
            if present {
                sql.push_str(condition);
            }
        }
        // `row_id` grows with the message id, and the index gives its matches in that order.
        sql.push_str(if expression.is_some() {
            " ORDER BY chat_search.rowid DESC LIMIT ?"
        } else {
            " ORDER BY m.id DESC LIMIT ?"
        });
        let mut query = sqlx::query(&sql)
            .bind(workspace_id.to_string())
            .bind(actor_id.to_string());
        if let Some(expression) = &expression {
            query = query.bind(expression);
        }
        for id in [input.conversation_id, input.author_id, input.cursor]
            .into_iter()
            .flatten()
        {
            query = query.bind(id.to_string());
        }
        let mut rows = query.bind(PAGE_SIZE as i64 + 1).fetch_all(&mut *tx).await?;
        let more = rows.len() > PAGE_SIZE;
        rows.truncate(PAGE_SIZE);

        let mut hits = Vec::with_capacity(rows.len());
        for row in &rows {
            let marked: String = row.get("marked");
            let ranges = if expression.is_some() {
                split_marks(&marked)
                    .1
                    .into_iter()
                    .map(|range| [range.start, range.end])
                    .collect()
            } else {
                Vec::new()
            };
            hits.push((parse_id(row.get("id"))?, ranges));
        }
        let records = sqlx::query(&format!(
            "{MESSAGE_SELECT} WHERE m.id IN (SELECT value FROM json_each(?))"
        ))
        .bind(id_list(hits.iter().map(|(id, _)| *id)))
        .fetch_all(&mut *tx)
        .await?;
        let mut messages = hydrate(&mut tx, &records).await?;
        let items: Vec<SearchHitRecord> = hits
            .into_iter()
            .filter_map(|(id, ranges)| {
                let index = messages.iter().position(|message| message.id == id)?;
                let message = messages.swap_remove(index);
                Some(SearchHitRecord {
                    conversation_id: message.conversation_id,
                    message,
                    ranges,
                })
            })
            .collect();
        let cursor = items.last().filter(|_| more).map(|hit| hit.message.id);
        Ok(SearchPage { items, cursor })
    }
}
