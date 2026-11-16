//! Release (spec §5.2, §5.3 step 4, §5.10): a held message whose time has come is built from its
//! draft, DKIM-signed, sealed and filed like received mail, and gets one recipient row for each
//! address to deliver to. Delivery comes after (the `mail.send` job).
//!
//! The reads, the MIME build and the signature happen before the writer transaction. The
//! transaction then claims the row with the same conditional `UPDATE` as
//! [`MailRepository::claim_release`] (`held` → `released` when due), also requiring the row's
//! `updated_at` that was read, so an Undo, an edit and a new Send in between make the claim fail
//! and the newer job releases the newer draft. The sealed blob, the receipt (`source = 'sent'`),
//! the message, the thread, the recipient rows, the sender's read state and the deletion of the
//! draft are written in that transaction: a failure leaves the row `held` and nothing else.
//!
//! **HTML policy.** The draft's HTML is sanitized again here with
//! [`orbit_mail::clean_outgoing_html`]: the `ammonia` defaults (no `style`, `class`, scripts or
//! event handlers; `http`, `https` and `mailto` links and remote images stay, as the author wrote
//! them) plus `cid:` URLs for the message's own inline images. The quoted original of a reply and
//! the original of an inline forward are sanitized by `orbit_mail` (`quote_reply`, `clean_html`).
//!
//! **Failures.** Without an active DKIM key at release the message is not sent unsigned: the row
//! stays `held` with `hold_reason = 'dkim_inactive'` (the Outbox shows it; Undo brings the draft
//! back). Other lasting problems hold it the same way (`send_as_denied`, `build_failed`,
//! `file_missing`, `storage_quota`). A draft that is gone (its address or thread was deleted)
//! cancels the row. Files sent as links (`as_link`) are left out of the message here.

use std::collections::{BTreeSet, HashMap};

use orbit_mail::{
    Composed, ComposedAddress, ComposedFile, ComposedFileKind, build_message, clean_html,
    clean_outgoing_html, forward_block, new_message_id, normalize_address, part_bytes, quote_reply,
    sign_message, trim_references,
};
use orbit_platform::{Id, Row, TimestampMillis};
use sha2::{Digest, Sha256};

use super::domains::parse_id;
use super::drafts::{Draft, DraftMode, load_draft};
use super::ingest::{Content, Stored, clean, insert_message};
use super::receipts::{NewReceipt, insert_receipt};
use super::search::people;
use super::signatures::check_sendable;
use super::threads::{Incoming, file_outgoing};
use super::{MailError, MailRepository, receipt_aad, seal_blob};

/// The longest name of a forwarded message's `.eml` file, in characters.
const EML_NAME_CHARS: usize = 100;

/// A message the release filed and queued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Released {
    pub outbound_id: Id,
    pub workspace_id: Id,
    pub mailbox_id: Id,
    pub thread_id: Id,
    pub message_id: Id,
    pub new_thread: bool,
    /// The users who read the mailbox, for `thread.updated`.
    pub users: Vec<Id>,
}

/// The `From` of the message.
struct Sender {
    address: String,
    name: Option<String>,
    domain_id: Id,
    domain: String,
}

/// The message a reply or forward answers.
struct Parent {
    message_id_header: Option<String>,
    references: Vec<String>,
    from_name: Option<String>,
    from_addr: Option<String>,
    to: Vec<String>,
    subject: String,
    /// Unix seconds.
    date: i64,
    body_text: String,
    html_part: Option<u32>,
    receipt_id: Id,
}

impl Parent {
    /// `Name <address>`, the address, or the name.
    fn sender_label(&self) -> String {
        match (&self.from_name, &self.from_addr) {
            (Some(name), Some(address)) if !name.trim().is_empty() => {
                format!("{} <{address}>", name.trim())
            }
            (_, Some(address)) => address.clone(),
            (Some(name), None) => name.clone(),
            (None, None) => String::new(),
        }
    }
}

/// A draft file as stored: where its bytes are.
struct FileRow {
    filename: String,
    content_type: String,
    content_id: Option<String>,
    as_link: bool,
    blob_id: String,
    storage_key: String,
    /// For a forwarded attachment: its part in the raw message of `receipt_id`.
    part_id: Option<i64>,
    receipt_id: Option<String>,
    forwarded: bool,
}

/// The distinct recipients with their kind, `to` before `cc` before `bcc`.
fn recipient_rows(draft: &Draft) -> Result<Vec<(String, &'static str)>, MailError> {
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    for (list, kind) in [(&draft.to, "to"), (&draft.cc, "cc"), (&draft.bcc, "bcc")] {
        for address in list {
            let address = normalize_address(address.trim())
                .map_err(|_| MailError::Unprocessable("invalid_address"))?
                .to_string();
            if seen.insert(address.clone()) {
                rows.push((address, kind));
            }
        }
    }
    if rows.is_empty() {
        return Err(MailError::Unprocessable("no_recipients"));
    }
    Ok(rows)
}

/// Header addresses (no display names: the draft stores bare addresses).
fn header_list(list: &[String]) -> Result<Vec<ComposedAddress>, MailError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for address in list {
        let address = normalize_address(address.trim())
            .map_err(|_| MailError::Unprocessable("invalid_address"))?
            .to_string();
        if seen.insert(address.clone()) {
            out.push(ComposedAddress {
                name: None,
                address,
            });
        }
    }
    Ok(out)
}

/// Text without control characters (a header value or a file name).
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The message-id as stored, with angle brackets.
fn bracketed(id: &str) -> String {
    let id = id.trim().trim_start_matches('<').trim_end_matches('>');
    format!("<{id}>")
}

async fn load_sender(
    connection: &mut sqlx::Transaction<'_, sqlx::Any>,
    address_id: Id,
    membership_id: Id,
) -> Result<Sender, MailError> {
    let row = sqlx::query(
        "SELECT a.local_part, d.id AS domain_id, d.name AS domain, mb.kind, mb.name AS mailbox_name, \
         u.display_name FROM mail_addresses a JOIN mail_domains d ON d.id = a.domain_id \
         JOIN mailboxes mb ON mb.id = a.mailbox_id JOIN memberships ms ON ms.id = $2 \
         JOIN users u ON u.id = ms.user_id WHERE a.id = $1",
    )
    .bind(address_id.to_string())
    .bind(membership_id.to_string())
    .fetch_optional(&mut **connection)
    .await?
    .ok_or(MailError::Unprocessable("send_as_denied"))?;
    let local: String = row.try_get("local_part")?;
    let domain: String = row.try_get("domain")?;
    let kind: String = row.try_get("kind")?;
    // A shared mailbox sends with its name, a personal one with its member's.
    let name: String = if kind == "shared" {
        row.try_get("mailbox_name")?
    } else {
        row.try_get("display_name")?
    };
    let name = plain(&name);
    Ok(Sender {
        address: format!("{local}@{domain}"),
        name: (!name.is_empty()).then_some(name),
        domain_id: parse_id(&row.try_get::<String, _>("domain_id")?)?,
        domain,
    })
}

async fn load_parent(
    connection: &mut sqlx::Transaction<'_, sqlx::Any>,
    message_id: Id,
) -> Result<Option<Parent>, MailError> {
    let Some(row) = sqlx::query(
        "SELECT message_id_header, reference_ids, from_name, from_addr, to_addrs, subject, \
         sent_at, received_at, body_text, html_part, receipt_id FROM mail_messages WHERE id = $1",
    )
    .bind(message_id.to_string())
    .fetch_optional(&mut **connection)
    .await?
    else {
        return Ok(None);
    };
    let references: String = row.try_get("reference_ids")?;
    let sent_at: Option<i64> = row.try_get("sent_at")?;
    let received_at: i64 = row.try_get("received_at")?;
    let html_part: Option<i64> = row.try_get("html_part")?;
    Ok(Some(Parent {
        message_id_header: row.try_get("message_id_header")?,
        references: serde_json::from_str(&references).unwrap_or_default(),
        from_name: row.try_get("from_name")?,
        from_addr: row.try_get("from_addr")?,
        to: people(&row.try_get::<String, _>("to_addrs")?)
            .into_iter()
            .map(|person| person.address)
            .collect(),
        subject: row.try_get("subject")?,
        date: sent_at.unwrap_or(received_at).div_euclid(1000),
        body_text: row.try_get("body_text")?,
        html_part: html_part.and_then(|part| u32::try_from(part).ok()),
        receipt_id: parse_id(&row.try_get::<String, _>("receipt_id")?)?,
    }))
}

async fn load_files(
    connection: &mut sqlx::Transaction<'_, sqlx::Any>,
    draft_id: Id,
) -> Result<Vec<FileRow>, MailError> {
    let rows = sqlx::query(
        "SELECT f.filename, f.content_type, f.content_id, f.as_link, f.blob_id, \
         f.source_attachment_id, b.storage_key, a.part_id, m.receipt_id \
         FROM mail_draft_files f JOIN attachment_blobs b ON b.id = f.blob_id \
         LEFT JOIN mail_attachments a ON a.id = f.source_attachment_id \
         LEFT JOIN mail_messages m ON m.id = a.message_id \
         WHERE f.draft_id = $1 ORDER BY f.created_at, f.id",
    )
    .bind(draft_id.to_string())
    .fetch_all(&mut **connection)
    .await?;
    rows.iter()
        .map(|row| {
            let source: Option<String> = row.try_get("source_attachment_id")?;
            Ok(FileRow {
                filename: row.try_get("filename")?,
                content_type: row.try_get("content_type")?,
                content_id: row.try_get("content_id")?,
                as_link: row.try_get::<i64, _>("as_link")? != 0,
                blob_id: row.try_get("blob_id")?,
                storage_key: row.try_get("storage_key")?,
                part_id: row.try_get("part_id")?,
                receipt_id: row.try_get("receipt_id")?,
                forwarded: source.is_some(),
            })
        })
        .collect()
}

impl MailRepository {
    /// Releases outbound message `outbound_id` when it is `held` and due at `now`: builds, signs,
    /// seals and files it (see the module docs) and returns what was filed. `None` when there is
    /// nothing to release (cancelled, already released, not due, or changed meanwhile). A lasting
    /// problem is an `Unprocessable` code that the job holds the row with
    /// ([`Self::hold_release`]); the row is then unchanged. `hostname` is the `HELO` recorded on
    /// the receipt (`orbit` without one).
    pub async fn release(
        &self,
        outbound_id: Id,
        hostname: Option<&str>,
        now: TimestampMillis,
    ) -> Result<Option<Released>, MailError> {
        let mut read = self.database.pool().begin().await?;
        let Some(row) = sqlx::query(
            "SELECT workspace_id, membership_id, mailbox_id, draft_id, updated_at FROM mail_outbound \
             WHERE id = $1 AND status = 'held' AND release_at <= $2",
        )
        .bind(outbound_id.to_string())
        .bind(now.as_millis())
        .fetch_optional(&mut *read)
        .await?
        else {
            return Ok(None);
        };
        let workspace_id = parse_id(&row.try_get::<String, _>("workspace_id")?)?;
        let membership_id = parse_id(&row.try_get::<String, _>("membership_id")?)?;
        let mailbox_id = parse_id(&row.try_get::<String, _>("mailbox_id")?)?;
        let draft_id: Option<String> = row.try_get("draft_id")?;
        let read_at: i64 = row.try_get("updated_at")?;
        let draft = match draft_id.map(|id| parse_id(&id)).transpose()? {
            Some(draft_id) => match load_draft(&mut read, membership_id, draft_id).await {
                Ok(draft) => Some(draft),
                Err(MailError::NotFound) => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        let Some(draft) = draft else {
            read.commit().await?;
            self.cancel_release(outbound_id, read_at, now).await?;
            return Ok(None);
        };
        if let Err(error) = check_sendable(
            &mut read,
            workspace_id,
            membership_id,
            draft.from_address_id,
        )
        .await
        {
            return Err(match error {
                MailError::NotFound => MailError::Unprocessable("send_as_denied"),
                other => other,
            });
        }
        let sender = load_sender(&mut read, draft.from_address_id, membership_id).await?;
        let parent = match draft.parent_message_id {
            Some(parent_id) => load_parent(&mut read, parent_id).await?,
            None => None,
        };
        let files = load_files(&mut read, draft.id).await?;
        let mut raw_refs = HashMap::new();
        for file in files.iter().filter(|file| !file.forwarded) {
            // A forwarded file whose attachment is gone still points at a sealed raw message.
            let sealed: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM mail_receipts WHERE blob_id = $1 LIMIT 1")
                    .bind(&file.blob_id)
                    .fetch_optional(&mut *read)
                    .await?;
            raw_refs.insert(file.blob_id.clone(), sealed.is_some());
        }
        read.commit().await?;

        let key = self
            .active_signing_key(sender.domain_id)
            .await?
            .ok_or(MailError::Unprocessable("dkim_inactive"))?;
        let mut raws: HashMap<Id, Vec<u8>> = HashMap::new();
        let mut composed_files = Vec::new();
        for file in files.iter().filter(|file| !file.as_link) {
            let bytes = if file.forwarded {
                let (Some(receipt), Some(part)) = (&file.receipt_id, file.part_id) else {
                    return Err(MailError::Unprocessable("file_missing"));
                };
                let receipt = parse_id(receipt)?;
                if let std::collections::hash_map::Entry::Vacant(entry) = raws.entry(receipt) {
                    entry.insert(self.read_raw(receipt).await?);
                }
                let part =
                    u32::try_from(part).map_err(|_| MailError::Unprocessable("file_missing"))?;
                part_bytes(&raws[&receipt], part).ok_or(MailError::Unprocessable("file_missing"))?
            } else {
                if raw_refs.get(&file.blob_id).copied().unwrap_or(false) {
                    return Err(MailError::Unprocessable("file_missing"));
                }
                self.uploads
                    .read_blob(&file.storage_key)
                    .await
                    .map_err(MailError::Upload)?
            };
            composed_files.push(ComposedFile {
                filename: plain(&file.filename),
                content_type: plain(&file.content_type),
                bytes,
                // A forwarded attachment stays an attachment, whatever Content-ID it had.
                kind: match &file.content_id {
                    Some(id) if !file.forwarded && !id.trim().is_empty() => {
                        ComposedFileKind::Inline {
                            content_id: id.trim().to_owned(),
                        }
                    }
                    _ => ComposedFileKind::Attachment,
                },
            });
        }

        let mut html = clean_outgoing_html(&draft.body_html);
        let mut in_reply_to = None;
        let mut references = Vec::new();
        if let Some(parent) = &parent {
            let parent_html = || async {
                match parent.html_part {
                    Some(part) => self.read_raw(parent.receipt_id).await.map(|raw| {
                        part_bytes(&raw, part)
                            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    }),
                    None => Ok(None),
                }
            };
            match draft.mode {
                DraftMode::Reply | DraftMode::ReplyAll => {
                    if let Some(id) = parent
                        .message_id_header
                        .as_deref()
                        .filter(|id| !id.is_empty())
                    {
                        let id = bracketed(id);
                        let refs: Vec<String> =
                            parent.references.iter().map(|id| bracketed(id)).collect();
                        references = trim_references(&refs, &id);
                        in_reply_to = Some(id);
                    }
                    let original = parent_html().await?;
                    let who = parent
                        .from_name
                        .clone()
                        .filter(|name| !name.trim().is_empty())
                        .or_else(|| parent.from_addr.clone())
                        .unwrap_or_default();
                    html.push_str(&quote_reply(
                        original.as_deref(),
                        &parent.body_text,
                        &who,
                        parent.date,
                    ));
                }
                DraftMode::Forward => {
                    let original = parent_html().await?;
                    html.push_str(&forward_block(
                        &parent.sender_label(),
                        parent.date,
                        &parent.subject,
                        &parent.to.join(", "),
                    ));
                    html.push_str(&match original.filter(|html| !html.trim().is_empty()) {
                        Some(original) => clean_html(&original),
                        None => escape_html(&parent.body_text).replace('\n', "<br>"),
                    });
                }
                DraftMode::ForwardAttached => {
                    let raw = match raws.remove(&parent.receipt_id) {
                        Some(raw) => raw,
                        None => self.read_raw(parent.receipt_id).await?,
                    };
                    let name: String = plain(&parent.subject)
                        .chars()
                        .take(EML_NAME_CHARS)
                        .collect();
                    composed_files.push(ComposedFile {
                        filename: format!(
                            "{}.eml",
                            if name.is_empty() {
                                "message"
                            } else {
                                name.as_str()
                            }
                        ),
                        content_type: "message/rfc822".to_owned(),
                        bytes: raw,
                        kind: ComposedFileKind::Message,
                    });
                }
                DraftMode::New => {}
            }
        }
        drop(raws);

        let recipients = recipient_rows(&draft)?;
        let from = ComposedAddress {
            name: sender.name.clone(),
            address: sender.address.clone(),
        };
        let mut to = header_list(&draft.to)?;
        let cc = header_list(&draft.cc)?;
        if to.is_empty() && cc.is_empty() {
            // Only Bcc: the header names the sender; the recipients are in the envelope.
            to.push(ComposedAddress {
                name: None,
                address: sender.address.clone(),
            });
        }
        let composed = Composed {
            message_id: new_message_id(&sender.domain),
            date: now.as_millis().div_euclid(1000),
            from,
            to,
            cc,
            reply_to: None,
            subject: plain(&draft.subject),
            in_reply_to,
            references,
            text: orbit_mail::html_to_text(&html),
            html,
            files: composed_files,
            extra_headers: Vec::new(),
        };
        let unsigned =
            build_message(&composed).map_err(|_| MailError::Unprocessable("build_failed"))?;
        drop(composed);
        let signed =
            sign_message(&unsigned, &key).map_err(|_| MailError::Unprocessable("build_failed"))?;
        drop(unsigned);
        let raw_sha256 = format!("{:x}", Sha256::digest(&signed));
        let size =
            i64::try_from(signed.len()).map_err(|_| MailError::Unprocessable("build_failed"))?;
        let content = Content::of(&signed);

        let finalization = self.uploads.begin_finalization().await;
        let mut transaction = self.database.immediate_transaction().await?;
        let claimed = sqlx::query(
            "UPDATE mail_outbound SET status = 'released', hold_reason = NULL, updated_at = $1 \
             WHERE id = $2 AND status = 'held' AND release_at <= $3 AND updated_at = $4",
        )
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .bind(now.as_millis())
        .bind(read_at)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if claimed != 1 {
            return Ok(None);
        }
        let receipt_id = Id::new_v7();
        let (key_version, data_key) = self.keys.current(&mut transaction, workspace_id).await?;
        let sealed = seal_blob(&data_key, &receipt_aad(receipt_id), &signed)?;
        drop(signed);
        let blob = finalization
            .finalize_bytes_in_transaction(&mut transaction, workspace_id, &sealed, now)
            .await?;
        drop(sealed);
        insert_receipt(
            &mut transaction,
            &NewReceipt {
                id: receipt_id,
                workspace_id,
                source: "sent",
                blob_id: blob.id,
                key_version,
                raw_sha256: &raw_sha256,
                size,
                mail_from: Some(&sender.address),
                peer_ip: String::new(),
                helo: hostname.unwrap_or("orbit"),
                tls: None,
                auth: "{}".to_owned(),
            },
            now,
        )
        .await?;
        sqlx::query("UPDATE mail_receipts SET status = 'ingested' WHERE id = $1")
            .bind(receipt_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let message_id = insert_message(
            &mut transaction,
            &Stored {
                receipt_id,
                workspace_id,
                raw_sha256: &raw_sha256,
                size,
                received_at: now,
            },
            &content,
            now,
        )
        .await?;

        // The sender's read state before the message: a thread read up to its last message (or
        // answered after it) stays read.
        let before = match draft.thread_id {
            Some(thread_id) => sqlx::query(
                "SELECT t.last_message_at, s.last_read_at FROM mail_threads t \
                 LEFT JOIN mail_thread_user_state s ON s.thread_id = t.id AND s.membership_id = $1 \
                 WHERE t.id = $2 AND t.mailbox_id = $3",
            )
            .bind(membership_id.to_string())
            .bind(thread_id.to_string())
            .bind(mailbox_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?
            .map(|row| {
                Ok::<_, MailError>((
                    row.try_get::<i64, _>("last_message_at")?,
                    row.try_get::<Option<i64>, _>("last_read_at")?,
                ))
            })
            .transpose()?,
            None => None,
        };
        let subject = clean(&content.parsed.subject);
        let normalized_subject = clean(&content.parsed.normalized_subject);
        let snippet = clean(&content.parsed.snippet);
        let incoming = Incoming {
            message_id,
            raw_sha256: &raw_sha256,
            thread_ids: &[],
            subject: &subject,
            normalized_subject: &normalized_subject,
            sender: Some(&sender.address),
            sender_inside: true,
            received_at: now,
            snippet: &snippet,
            has_attachments: content.has_attachments(),
            dmarc_failed: false,
        };
        let filed = file_outgoing(
            &mut transaction,
            workspace_id,
            mailbox_id,
            draft.thread_id,
            &incoming,
            now,
        )
        .await?
        .ok_or(MailError::Conflict("already_filed"))?;
        let read = match before {
            None => true,
            Some((last_message_at, last_read_at)) => {
                last_read_at.is_some_and(|at| at >= last_message_at)
                    || draft.updated_at.as_millis() >= last_message_at
            }
        };
        if read {
            sqlx::query(
                "INSERT INTO mail_thread_user_state (thread_id, membership_id, last_read_at, starred) \
                 SELECT id, $1, last_message_at, 0 FROM mail_threads WHERE id = $2 \
                 ON CONFLICT (thread_id, membership_id) DO UPDATE SET last_read_at = excluded.last_read_at",
            )
            .bind(membership_id.to_string())
            .bind(filed.thread_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        for (address, kind) in &recipients {
            sqlx::query(
                "INSERT INTO mail_outbound_recipients (id, outbound_id, workspace_id, membership_id, \
                 address, kind, status, released_at, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, 'queued', $7, $7, $7)",
            )
            .bind(Id::new_v7().to_string())
            .bind(outbound_id.to_string())
            .bind(workspace_id.to_string())
            .bind(membership_id.to_string())
            .bind(address)
            .bind(*kind)
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query("UPDATE mail_outbound SET message_id = $1, draft_id = NULL WHERE id = $2")
            .bind(message_id.to_string())
            .bind(outbound_id.to_string())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE mail_send_policies SET first_release_at = $1, updated_at = $2 \
             WHERE workspace_id = $3 AND first_release_at IS NULL",
        )
        .bind(now.as_millis())
        .bind(now.as_millis())
        .bind(workspace_id.to_string())
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM mail_drafts WHERE id = $1")
            .bind(draft.id.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        drop(finalization);
        Ok(Some(Released {
            outbound_id,
            workspace_id,
            mailbox_id,
            thread_id: filed.thread_id,
            message_id,
            new_thread: filed.new_thread,
            users: filed.users,
        }))
    }

    /// Keeps a held message waiting with `reason` (shown in the Outbox) after a release failed.
    /// Undo still brings its draft back.
    pub async fn hold_release(
        &self,
        outbound_id: Id,
        reason: &str,
        now: TimestampMillis,
    ) -> Result<(), MailError> {
        sqlx::query(
            "UPDATE mail_outbound SET hold_reason = $1, updated_at = $2 \
             WHERE id = $3 AND status = 'held'",
        )
        .bind(reason)
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    /// A held message whose draft is gone cannot be built: it is cancelled.
    async fn cancel_release(
        &self,
        outbound_id: Id,
        read_at: i64,
        now: TimestampMillis,
    ) -> Result<(), MailError> {
        sqlx::query(
            "UPDATE mail_outbound SET status = 'cancelled', hold_reason = 'draft_missing', \
             updated_at = $1 WHERE id = $2 AND status = 'held' AND updated_at = $3",
        )
        .bind(now.as_millis())
        .bind(outbound_id.to_string())
        .bind(read_at)
        .execute(self.database.pool())
        .await?;
        Ok(())
    }
}
