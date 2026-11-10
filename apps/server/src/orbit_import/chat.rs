//! The chat side of an import. Public channels (full import only) are matched by name, else
//! created; private channels are created with the members matched by email and the importer;
//! a DM becomes a real DM when every participant is here (the importer among them), else a
//! private channel named after the people. Messages are history: they are written directly,
//! in order, with their replies, reactions and files, and nothing fires on them (no
//! notification, mention row, push, bot, webhook or live event). Every member's read marker
//! ends at the last message, so nothing shows as unread. A personal import's archive is the
//! importer's own upload: every message is written by the importer with a name and time header,
//! only the importer's reactions are kept, and a conversation a full import created is skipped.

use std::collections::HashSet;

use orbit_platform::{Id, TimestampMillis};

use super::archive::{self, Archive};
use super::model::{self, Conversation, Message};
use super::tasks::{author_header, read_file, source_time};
use super::{Item, OrbitImportService, Run, record_link_in};
use crate::notion::files::sanitize_file_name;
use crate::notion::import::clamp;
use crate::repositories::chat::{ChannelCreate, ChatError, ConversationKind, MESSAGE_MAX_CHARS};
use crate::task_import::Failure;
use crate::workspace_export::ExportMode;

const NAME_MAX_CHARS: usize = 80;
/// Authors shown on a thread's summary row.
const SUMMARY_AUTHORS: usize = 3;

/// `DM - Ann, Bob` for a DM, else the channel's name.
pub(super) fn conversation_title(conversation: &Conversation) -> String {
    if conversation.kind == "dm" {
        let names: Vec<String> = conversation
            .members
            .iter()
            .map(model::Person::display)
            .collect();
        format!("DM - {}", names.join(", "))
    } else {
        conversation.name.clone()
    }
}

/// Lists the selected conversations. Blocking reads.
pub(super) async fn plan_channels(run: &Run) -> Result<Vec<Item>, Failure> {
    let selected = run.selection.channel_ids.clone();
    let personal = run.mode == ExportMode::Personal;
    let listed = archive::blocking(&run.archive, move |archive| {
        let mut channels = Vec::new();
        for path in archive.paths("chat/", ".json") {
            let conversation: Conversation = archive.read_json(&path)?;
            if conversation.id.is_empty()
                || !selected.contains(&conversation.id)
                || (personal && conversation.kind == "public")
            {
                continue;
            }
            channels.push((
                conversation.id.clone(),
                path,
                conversation_title(&conversation),
            ));
        }
        channels.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(channels)
    })
    .await?;
    Ok(listed
        .into_iter()
        .map(|(id, path, title)| Item {
            kind: "channel",
            source_id: id,
            parent_id: None,
            path,
            title: clamp(&title, 500, 2_000),
            orbit_id: None,
            state: "pending".to_owned(),
        })
        .collect())
}

pub(super) async fn import_channels(
    service: &OrbitImportService,
    run: &mut Run,
) -> Result<(), Failure> {
    for mut item in service.load_items(run.import_id, "channel").await? {
        if item.state == "filled"
            || item.state == "failed"
            || service.skip_if_taken_by_full_import(run, &mut item).await?
        {
            continue;
        }
        let filled = service
            .step(run, &mut item, async |service, run, item| {
                import_channel(service, run, item).await?;
                service.set_item_state(run, item, "filled").await
            })
            .await?;
        if filled {
            service.save_report(run).await?;
        }
    }
    Ok(())
}

async fn import_channel(
    service: &OrbitImportService,
    run: &mut Run,
    item: &mut Item,
) -> Result<(), Failure> {
    let path = item.path.clone();
    let conversation: Conversation = archive::blocking(&run.archive, move |archive: &Archive| {
        archive.read_json(&path)
    })
    .await
    .map_err(|error| Failure::Item(error.to_string()))?;
    let members = matched_members(run, &conversation.members);
    let conversation_id = ensure_conversation(service, run, item, &conversation, &members).await?;
    service.set_item_state(run, item, "created").await?;
    if conversation.kind != "public" {
        ensure_members(service, run, conversation_id, &members).await?;
    }
    for root in &conversation.messages {
        if service.should_stop(run).await? {
            return crate::task_import::stopped(&run.job);
        }
        insert_thread(service, run, conversation_id, root).await?;
    }
    refresh_conversation(service, conversation_id).await?;
    Ok(())
}

/// The members here that the conversation's members match, the importer among them.
fn matched_members(run: &mut Run, people: &[model::Person]) -> Vec<Id> {
    let mut members = vec![run.user_id];
    for person in people {
        if let Some(id) = run.member(&person.id, person.name.as_deref())
            && !members.contains(&id)
        {
            members.push(id);
        }
    }
    members
}

/// The conversation an earlier import made, else one created now.
async fn ensure_conversation(
    service: &OrbitImportService,
    run: &mut Run,
    item: &mut Item,
    conversation: &Conversation,
    members: &[Id],
) -> Result<Id, Failure> {
    if let Some(id) = item.orbit_id {
        return Ok(id);
    }
    let linked = service.linked(run, "channel", &conversation.id).await?;
    let id = match linked {
        Some(id) => {
            run.report.skipped_existing += 1;
            id
        }
        None => {
            let (id, created) = create_conversation(service, run, conversation, members).await?;
            if created {
                run.report.created.channels += 1;
            } else {
                run.report.skipped_existing += 1;
            }
            id
        }
    };
    sqlx::query(
        "UPDATE orbit_import_items SET orbit_id = $1 WHERE import_id = $2 AND kind = 'channel' AND source_id = $3",
    )
    .bind(id.to_string())
    .bind(run.import_id.to_string())
    .bind(&conversation.id)
    .execute(service.database().pool())
    .await?;
    service
        .record_link(run, "channel", &conversation.id, id)
        .await?;
    item.orbit_id = Some(id);
    Ok(id)
}

/// Returns the conversation and whether it was created now (a public channel with the same
/// name here is reused).
async fn create_conversation(
    service: &OrbitImportService,
    run: &mut Run,
    conversation: &Conversation,
    members: &[Id],
) -> Result<(Id, bool), Failure> {
    let others: Vec<Id> = members
        .iter()
        .copied()
        .filter(|id| *id != run.user_id)
        .collect();
    let chat_error = |error: ChatError| match error {
        ChatError::Unavailable(error) => Failure::from(error),
        other => Failure::Item(format!("Orbit refused the channel: {other}.")),
    };
    match conversation.kind.as_str() {
        "public" if run.mode == ExportMode::Full => {
            let name = channel_name(&conversation.name, "imported");
            if let Some(existing) = channel_by_name(service, run, &name).await? {
                return Ok((existing, false));
            }
            let created = service
                .inner
                .chat
                .create_channel(
                    run.workspace_id,
                    run.user_id,
                    ChannelCreate {
                        name,
                        topic: clamp(
                            conversation.topic.as_deref().unwrap_or_default(),
                            250,
                            1_000,
                        ),
                        category_id: None,
                        kind: ConversationKind::Public,
                        member_ids: Vec::new(),
                    },
                )
                .await
                .map_err(chat_error)?;
            Ok((created.value.id, true))
        }
        "public" => Err(Failure::Item(
            "A personal import holds no public channels.".to_owned(),
        )),
        "dm" if others.len() + 1 == conversation.members.len()
            && conversation.members.iter().any(|person| {
                run.people
                    .get(&person.id)
                    .and_then(|source| source.email.as_deref())
                    .and_then(|email| run.members.get(&email.trim().to_lowercase()))
                    == Some(&run.user_id)
            }) =>
        {
            let created = service
                .inner
                .chat
                .open_dm(run.workspace_id, run.user_id, others)
                .await
                .map_err(chat_error)?;
            Ok((created.value.id, true))
        }
        kind => {
            let base = if kind == "dm" {
                let names: Vec<String> = conversation
                    .members
                    .iter()
                    .map(|person| run.name_of(&person.id, person.name.as_deref()))
                    .collect();
                format!("dm-{}-imported", names.join("-"))
            } else {
                conversation.name.clone()
            };
            let name = free_channel_name(service, run, &channel_name(&base, "imported")).await?;
            let created = service
                .inner
                .chat
                .create_channel(
                    run.workspace_id,
                    run.user_id,
                    ChannelCreate {
                        name,
                        topic: clamp(
                            conversation.topic.as_deref().unwrap_or_default(),
                            250,
                            1_000,
                        ),
                        category_id: None,
                        kind: ConversationKind::Private,
                        member_ids: others,
                    },
                )
                .await
                .map_err(chat_error)?;
            Ok((created.value.id, true))
        }
    }
}

/// A channel name as Orbit stores them: lower case, `-` for spaces, at most 80 characters.
fn channel_name(raw: &str, fallback: &str) -> String {
    let name: String = raw
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_MAX_CHARS)
        .collect();
    if name.is_empty() {
        fallback.to_owned()
    } else {
        name
    }
}

async fn channel_by_name(
    service: &OrbitImportService,
    run: &Run,
    name: &str,
) -> Result<Option<Id>, Failure> {
    let found: Option<String> = sqlx::query_scalar(
        "SELECT id FROM chat_conversations WHERE workspace_id = $1 AND name = $2 AND kind <> 'dm' \
         AND archived_at IS NULL",
    )
    .bind(run.workspace_id.to_string())
    .bind(name)
    .fetch_optional(service.database().pool())
    .await?;
    Ok(found.and_then(|id| id.parse().ok()))
}

/// `name`, else `name-2`, `name-3`, … while the name is taken.
async fn free_channel_name(
    service: &OrbitImportService,
    run: &Run,
    name: &str,
) -> Result<String, Failure> {
    if channel_by_name(service, run, name).await?.is_none() {
        return Ok(name.to_owned());
    }
    for attempt in 2..100 {
        let suffix = format!("-{attempt}");
        let base: String = name
            .chars()
            .take(NAME_MAX_CHARS.saturating_sub(suffix.len()))
            .collect();
        let candidate = format!("{base}{suffix}");
        if channel_by_name(service, run, &candidate).await?.is_none() {
            return Ok(candidate);
        }
    }
    Err(Failure::Item("No free channel name was found.".to_owned()))
}

/// Adds the matched members (and the importer) to a private channel they are not in yet.
async fn ensure_members(
    service: &OrbitImportService,
    run: &Run,
    conversation_id: Id,
    members: &[Id],
) -> Result<(), Failure> {
    let kind: Option<String> =
        sqlx::query_scalar("SELECT kind FROM chat_conversations WHERE id = $1")
            .bind(conversation_id.to_string())
            .fetch_optional(service.database().pool())
            .await?;
    // A real DM has exactly its participants; a public channel has everyone.
    if kind.as_deref() != Some("private") {
        return Ok(());
    }
    let now = TimestampMillis::now().as_millis();
    for member in members {
        sqlx::query(
            "INSERT INTO chat_members (conversation_id, user_id, workspace_id, notify, read_count, joined_at) \
             VALUES ($1, $2, $3, 'mentions', 0, $4) ON CONFLICT (conversation_id, user_id) DO NOTHING",
        )
        .bind(conversation_id.to_string())
        .bind(member.to_string())
        .bind(run.workspace_id.to_string())
        .bind(now)
        .execute(service.database().pool())
        .await?;
    }
    Ok(())
}

/// A root message with its replies. System rows (`pin`, `join`, `leave`) and deleted messages
/// without replies are left out.
async fn insert_thread(
    service: &OrbitImportService,
    run: &mut Run,
    conversation_id: Id,
    root: &Message,
) -> Result<(), Failure> {
    if root.kind != "message" || root.id.is_empty() {
        return Ok(());
    }
    let replies: Vec<&Message> = root
        .replies
        .iter()
        .filter(|reply| reply.kind == "message" && !reply.deleted && !reply.id.is_empty())
        .collect();
    if root.deleted && replies.is_empty() {
        return Ok(());
    }
    let root_id = match service.linked(run, "message", &root.id).await? {
        Some(id) => id,
        None => insert_message(service, run, conversation_id, root, None).await?,
    };
    let mut added = false;
    for reply in replies {
        if service.linked(run, "message", &reply.id).await?.is_some() {
            continue;
        }
        insert_message(service, run, conversation_id, reply, Some(root_id)).await?;
        added = true;
    }
    if added || !root.replies.is_empty() {
        refresh_thread(service, root_id).await?;
    }
    Ok(())
}

/// One message with its reactions and files, in one transaction with its link.
async fn insert_message(
    service: &OrbitImportService,
    run: &mut Run,
    conversation_id: Id,
    message: &Message,
    thread_root_id: Option<Id>,
) -> Result<Id, Failure> {
    let author = message
        .author
        .as_ref()
        .and_then(|person| run.member(&person.id, person.name.as_deref()));
    let author_name = match (&message.bot_name, &message.author) {
        (Some(bot), _) => bot.clone(),
        (None, Some(person)) => run.name_of(&person.id, person.name.as_deref()),
        (None, None) => "Unknown member".to_owned(),
    };
    let text = rewrite_mentions(service, run, &message.body).await?;
    // A personal archive is the importer's own upload, so nothing in it is written as, or
    // reacted to by, anyone else.
    let personal = run.mode == ExportMode::Personal;
    let (author_id, body) = match (author, &message.bot_name) {
        (Some(id), None) if !personal => (id, text),
        _ => {
            let header = author_header(&author_name, &message.created_at);
            (
                run.user_id,
                if text.trim().is_empty() {
                    header
                } else {
                    format!("{header}\n\n{text}")
                },
            )
        }
    };
    let body: String = body.chars().take(MESSAGE_MAX_CHARS).collect();
    let created_at = source_time(&message.created_at);
    let reply_to = match &message.reply_to_id {
        Some(source) => service.linked(run, "message", source).await?,
        None => None,
    };
    let forward_of = match &message.forward_of_id {
        Some(source) => service.linked(run, "message", source).await?,
        None => None,
    };
    let mut reactions: Vec<(String, Id)> = Vec::new();
    for reaction in &message.reactions {
        let emoji = reaction.emoji.trim();
        if emoji.is_empty() || emoji.len() > 64 {
            continue;
        }
        for person in &reaction.users {
            if let Some(id) = run.member(&person.id, person.name.as_deref())
                && (!personal || id == run.user_id)
            {
                reactions.push((emoji.to_owned(), id));
            }
        }
    }

    // Files are staged before the transaction; the blobs are installed inside it.
    let mut staged = Vec::new();
    for file in &message.files {
        if service
            .linked(run, "message_file", &file.id)
            .await?
            .is_some()
        {
            continue;
        }
        let Some(path) = file.path.as_deref() else {
            continue;
        };
        let Some(bytes) = read_file(run, path, service.inner.settings.max_file_bytes).await? else {
            continue;
        };
        let name = sanitize_file_name(&file.name);
        let name = if name.is_empty() {
            "file".to_owned()
        } else {
            name
        };
        match service
            .inner
            .uploads
            .stage(run.workspace_id, run.user_id, &name, &bytes[..])
            .await
        {
            Ok(upload) => staged.push((file.id.clone(), upload)),
            Err(error) => tracing::debug!(error = %error, "imported chat file could not be staged"),
        }
    }

    let result = write_message(
        service,
        run,
        NewImportedMessage {
            conversation_id,
            thread_root_id,
            author_id,
            body: &body,
            message,
            created_at,
            reply_to,
            forward_of,
            reactions: &reactions,
            staged: &staged,
        },
    )
    .await;
    if result.is_err() {
        for (_, upload) in &staged {
            let _ = service.inner.uploads.discard(upload).await;
        }
    }
    let id = result?;
    run.report.created.messages += 1;
    run.report.created.files += staged.len() as u64;
    Ok(id)
}

struct NewImportedMessage<'a> {
    conversation_id: Id,
    thread_root_id: Option<Id>,
    author_id: Id,
    body: &'a str,
    message: &'a Message,
    created_at: TimestampMillis,
    reply_to: Option<Id>,
    forward_of: Option<Id>,
    reactions: &'a [(String, Id)],
    staged: &'a [(String, orbit_platform::StagedUpload)],
}

async fn write_message(
    service: &OrbitImportService,
    run: &Run,
    new: NewImportedMessage<'_>,
) -> Result<Id, Failure> {
    let finalization = service.inner.uploads.begin_finalization().await;
    let now = service.database().database_now().await?;
    let mut tx = service.database().immediate_transaction().await?;
    let id = Id::new_v7();
    let message = new.message;
    sqlx::query(
        "INSERT INTO chat_messages (id, conversation_id, thread_root_id, kind, author_id, body, \
         pinned_at, pinned_by, edited_at, deleted_at, created_at, reply_to_id, forward_of_id) \
         VALUES ($1, $2, $3, 'message', $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(id.to_string())
    .bind(new.conversation_id.to_string())
    .bind(new.thread_root_id.map(|id| id.to_string()))
    .bind(new.author_id.to_string())
    .bind(new.body)
    .bind(message.pinned.then_some(new.created_at.as_millis()))
    .bind(message.pinned.then(|| new.author_id.to_string()))
    .bind(
        message
            .edited_at
            .as_deref()
            .and_then(crate::task_import::parse_instant_millis),
    )
    .bind(message.deleted.then_some(new.created_at.as_millis()))
    .bind(new.created_at.as_millis())
    .bind(new.reply_to.map(|id| id.to_string()))
    .bind(new.forward_of.map(|id| id.to_string()))
    .execute(&mut *tx)
    .await?;
    record_link_in(
        &mut *tx,
        run.workspace_id,
        &run.source_workspace_id,
        &run.link_scope,
        "message",
        &message.id,
        id,
    )
    .await?;
    for (emoji, user_id) in new.reactions {
        sqlx::query(
            "INSERT INTO chat_reactions (message_id, emoji, user_id, created_at) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (message_id, emoji, user_id) DO NOTHING",
        )
        .bind(id.to_string())
        .bind(emoji)
        .bind(user_id.to_string())
        .bind(new.created_at.as_millis())
        .execute(&mut *tx)
        .await?;
    }
    for (source_file_id, upload) in new.staged {
        let blob = finalization
            .finalize_blob_in_transaction(&mut tx, upload, now)
            .await
            .map_err(|error| Failure::Transient(error.to_string()))?;
        let file_id = Id::new_v7();
        sqlx::query(
            "INSERT INTO chat_message_files (id, workspace_id, message_id, conversation_id, blob_id, \
             file_name, mime_type, size_bytes, uploaded_by, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(file_id.to_string())
        .bind(run.workspace_id.to_string())
        .bind(id.to_string())
        .bind(new.conversation_id.to_string())
        .bind(blob.id.to_string())
        .bind(&upload.display_name)
        .bind(&upload.detected_media_type)
        .bind(i64::try_from(upload.size_bytes).unwrap_or(i64::MAX))
        .bind(new.author_id.to_string())
        .bind(new.created_at.as_millis())
        .execute(&mut *tx)
        .await?;
        record_link_in(
            &mut *tx,
            run.workspace_id,
            &run.source_workspace_id,
            &run.link_scope,
            "message_file",
            source_file_id,
            file_id,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(id)
}

/// `<@id>` tokens point at the matched member here, else become `@Name`; `<#id>` tokens point
/// at the imported channel, else become `#channel`.
async fn rewrite_mentions(
    service: &OrbitImportService,
    run: &mut Run,
    body: &str,
) -> Result<String, Failure> {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let token = &rest[start..];
        let Some(end) = token.find('>') else {
            out.push_str(token);
            return Ok(out);
        };
        let inner = &token[1..end];
        if let Some(id) = inner.strip_prefix('@') {
            match run.member(id, None) {
                Some(member) => out.push_str(&format!("<@{member}>")),
                None => out.push_str(&format!("@{}", run.name_of(id, None))),
            }
        } else if let Some(id) = inner.strip_prefix('#') {
            match service.linked(run, "channel", id).await? {
                Some(channel) => out.push_str(&format!("<#{channel}>")),
                None => out.push_str("#channel"),
            }
        } else {
            out.push_str(&token[..=end]);
        }
        rest = &token[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The root's thread summary from its replies.
async fn refresh_thread(service: &OrbitImportService, root_id: Id) -> Result<(), Failure> {
    let authors: Vec<String> = sqlx::query_scalar(
        "SELECT author_id FROM chat_messages WHERE thread_root_id = $1 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(root_id.to_string())
    .fetch_all(service.database().pool())
    .await?;
    let mut seen = HashSet::new();
    let summary: Vec<String> = authors
        .into_iter()
        .filter(|author| seen.insert(author.clone()))
        .take(SUMMARY_AUTHORS)
        .collect();
    sqlx::query(
        "UPDATE chat_messages SET \
         reply_count = (SELECT COUNT(*) FROM chat_messages r WHERE r.thread_root_id = $1 AND r.deleted_at IS NULL), \
         last_reply_id = (SELECT MAX(r.id) FROM chat_messages r WHERE r.thread_root_id = $1), \
         reply_user_ids = $2 WHERE id = $1",
    )
    .bind(root_id.to_string())
    .bind(serde_json::to_string(&summary).unwrap_or_else(|_| "[]".to_owned()))
    .execute(service.database().pool())
    .await?;
    Ok(())
}

/// The conversation's counters from its messages, and every member's read marker at the end.
async fn refresh_conversation(
    service: &OrbitImportService,
    conversation_id: Id,
) -> Result<(), Failure> {
    let mut tx = service.database().immediate_transaction().await?;
    sqlx::query(
        "UPDATE chat_conversations SET \
         message_count = (SELECT COUNT(*) FROM chat_messages m WHERE m.conversation_id = $1 AND m.kind = 'message' \
                          AND m.deleted_at IS NULL AND (m.thread_root_id IS NULL OR m.also_in_channel = 1)), \
         last_message_at = (SELECT MAX(m.created_at) FROM chat_messages m WHERE m.conversation_id = $1), \
         updated_at = $2 WHERE id = $1",
    )
    .bind(conversation_id.to_string())
    .bind(TimestampMillis::now().as_millis())
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE chat_members SET \
         read_count = (SELECT message_count FROM chat_conversations c WHERE c.id = $1), \
         last_read_message_id = (SELECT m.id FROM chat_messages m WHERE m.conversation_id = $1 ORDER BY m.id DESC LIMIT 1) \
         WHERE conversation_id = $1",
    )
    .bind(conversation_id.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
