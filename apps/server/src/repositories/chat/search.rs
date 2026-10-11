//! Message search (FTS5, migration 0037) over the conversations the caller can read. Every word
//! must match (the last one as a prefix); case and diacritics are ignored. Newest message first,
//! paged by message id.

use orbit_platform::Id;
use orbit_platform::Row;
use serde::Serialize;
use utoipa::ToSchema;

use super::{
    ChatError, ChatRepository, Dialect, MESSAGE_SELECT, MessageRecord, hydrate, id_list, in_ids,
    load_access, load_actor, parse_id,
};
use crate::repositories::pages::{fts_query, pg_tsquery, split_marks};

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
        let postgres = self.database.postgres();
        let expression = if postgres {
            pg_tsquery(&input.query)
        } else {
            fts_query(&input.query)
        };
        let filtered =
            input.conversation_id.is_some() || input.author_id.is_some() || input.has_file;
        if expression.is_none() && !filtered {
            return Ok(SearchPage {
                items: Vec::new(),
                cursor: None,
            });
        }

        // Placeholders follow the bind order: workspace, actor, [expression], [conversation],
        // [author], [cursor], limit.
        let mut next = 3;
        let mut param = || {
            next += 1;
            format!("${}", next - 1)
        };
        let expression_param = expression.as_ref().map(|_| param());
        let mut sql = match &expression_param {
            Some(n) if postgres => format!(
                "SELECT m.id, ts_headline('orbit_search', chat_search.body, to_tsquery('orbit_search', {n}), \
                 'HighlightAll=true, StartSel=' || chr(2) || ', StopSel=' || chr(3)) AS marked \
                 FROM chat_search JOIN chat_messages m ON m.row_id = chat_search.rowid"
            ),
            Some(_) => "SELECT m.id, highlight(chat_search, 0, char(2), char(3)) AS marked \
                 FROM chat_search JOIN chat_messages m ON m.row_id = chat_search.rowid"
                .to_owned(),
            None => "SELECT m.id, m.body AS marked FROM chat_messages m".to_owned(),
        };
        sql.push_str(
            " JOIN chat_conversations c ON c.id = m.conversation_id \
             WHERE c.workspace_id = $1 AND m.kind = 'message' AND m.deleted_at IS NULL \
             AND (c.kind = 'public' OR EXISTS (SELECT 1 FROM chat_members \
                  WHERE chat_members.conversation_id = c.id AND chat_members.user_id = $2))",
        );
        if let Some(n) = &expression_param {
            sql.push_str(&if postgres {
                format!(" AND chat_search.search_vector @@ to_tsquery('orbit_search', {n})")
            } else {
                format!(" AND chat_search MATCH {n}")
            });
        }
        if input.conversation_id.is_some() {
            sql.push_str(&format!(" AND m.conversation_id = {}", param()));
        }
        if input.author_id.is_some() {
            sql.push_str(&format!(" AND m.author_id = {}", param()));
        }
        if input.has_file {
            sql.push_str(" AND EXISTS (SELECT 1 FROM chat_message_files WHERE message_id = m.id)");
        }
        if input.cursor.is_some() {
            sql.push_str(&format!(" AND m.id < {}", param()));
        }
        // `row_id` grows with the message id, and the index gives its matches in that order.
        let limit = param();
        sql.push_str(&if expression.is_some() {
            format!(" ORDER BY chat_search.rowid DESC LIMIT {limit}")
        } else {
            format!(" ORDER BY m.id DESC LIMIT {limit}")
        });
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql))
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
        let records = sqlx::query(sqlx::AssertSqlSafe(format!(
            "{MESSAGE_SELECT} WHERE m.id {}",
            in_ids(&*tx, 1)
        )))
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
