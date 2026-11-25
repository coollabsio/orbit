//! Vacation replies (spec §5.11).
//!
//! **Settings.** One row for each mailbox (`mail_vacations`): a personal mailbox is set by its
//! owner, a shared one by members with `mail.manage` (other members of a shared mailbox may
//! read it). `starts_at` and `ends_at` are milliseconds. The window is **inclusive at the start
//! and exclusive at the end**: a reply is sent for mail received at `t` with
//! `starts_at <= t < ends_at`; an empty bound is open. The API takes an RFC 3339 instant
//! (`2026-11-20T00:00:00+02:00`) or a local date (`2026-11-20`) or date and time without an
//! offset (`2026-11-20T09:00`), read in the time zone of the mailbox owner (a personal mailbox)
//! or of the member who saves (a shared one); UTC when none is set.
//!
//! **Reply.** After ingest files a message into a thread in the inbox,
//! [`MailRepository::maybe_vacation_reply`] decides with [`orbit_mail::vacation_allowed`] (RFC
//! 3834) and, when allowed, queues a normal outbound without a draft (`draft_id` NULL, no undo
//! delay) whose `auto_reply` JSON holds what [`MailRepository::release`] builds it from, marked
//! `automatic` (its bounces and complaints never pause the workspace or move the thread). Only
//! an authenticated envelope sender gets one ([`orbit_mail::envelope_authenticated`], or a
//! sender of this instance): no backscatter to forged senders. The
//! check of the 7-day rule and the record in `mail_vacation_replies` are one writer
//! transaction, so two messages of one sender that ingest at once get one reply.

use jiff::tz::TimeZone;
use orbit_domain::Permission;
use orbit_mail::{
    AuthOutcome, DmarcVerdict, VacationInput, envelope_authenticated, vacation_allowed,
};
use orbit_platform::{Id, Row, TimestampMillis};
use serde::{Deserialize, Serialize};
use sqlx::{Any, Transaction};

use super::domains::parse_id;
use super::limits::{paused, route_limited};
use super::outbound::{enqueue, send_job};
use super::search::people;
use super::{MailError, MailRepository, SendReadiness};
use crate::repositories::membership;

/// One reply to a sender in this time.
pub const REPLY_INTERVAL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const SUBJECT_MAX_CHARS: usize = 200;
const BODY_MAX_BYTES: usize = 20_000;

/// A mailbox's vacation settings as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vacation {
    pub mailbox_id: Id,
    pub enabled: bool,
    /// Inclusive.
    pub starts_at: Option<TimestampMillis>,
    /// Exclusive.
    pub ends_at: Option<TimestampMillis>,
    /// Empty: the reply is `Re: ` and the subject of the message.
    pub subject: String,
    pub body_html: String,
    /// Only reply to people the member (or, for a shared mailbox, any member) wrote to.
    pub contacts_only: bool,
    /// The zone that dates without an offset are read in.
    pub timezone: String,
}

/// New settings. `starts_at` and `ends_at` are text (see the module docs).
#[derive(Clone, Debug, Default)]
pub struct VacationChange {
    pub enabled: bool,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub subject: String,
    pub body_html: String,
    pub contacts_only: bool,
}

/// What a vacation outbound is built from at release (`mail_outbound.auto_reply`).
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct AutoReply {
    pub(super) parent_message_id: String,
    pub(super) thread_id: String,
    pub(super) from_address_id: String,
    pub(super) to: String,
    pub(super) subject: String,
    pub(super) body_html: String,
}

fn moment(text: &str, zone: &TimeZone, field: &'static str) -> Result<i64, MailError> {
    let text = text.trim();
    if let Ok(instant) = text.parse::<jiff::Timestamp>() {
        return Ok(instant.as_millisecond());
    }
    let civil = text
        .parse::<jiff::civil::DateTime>()
        .or_else(|_| {
            text.parse::<jiff::civil::Date>()
                .map(|date| date.to_datetime(jiff::civil::Time::midnight()))
        })
        .map_err(|_| MailError::Invalid { field })?;
    zone.to_timestamp(civil)
        .map(|instant| instant.as_millisecond())
        .map_err(|_| MailError::Invalid { field })
}

/// The header fields of a raw message, unfolded, as `(name, value)`.
fn header_pairs(raw: &[u8]) -> Vec<(String, String)> {
    let end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|window| window == b"\n\n"))
        .unwrap_or(raw.len());
    let block = String::from_utf8_lossy(&raw[..end]);
    let mut fields: Vec<String> = Vec::new();
    for line in block.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = fields.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        } else {
            fields.push(line.to_owned());
        }
    }
    fields
        .into_iter()
        .filter_map(|field| {
            let (name, value) = field.split_once(':')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

struct Access {
    membership_id: Id,
    personal: bool,
    owner: Option<Id>,
    may_write: bool,
}

/// What the caller may do with the vacation of `mailbox_id` (`NotFound` for a mailbox they
/// cannot see).
async fn access(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    user_id: Id,
    mailbox_id: Id,
) -> Result<Access, MailError> {
    let actor = membership::actor(&mut **transaction, workspace_id, user_id)
        .await?
        .ok_or(MailError::NotFound)?;
    let membership_id: String =
        sqlx::query_scalar("SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2")
            .bind(workspace_id.to_string())
            .bind(user_id.to_string())
            .fetch_one(&mut **transaction)
            .await?;
    let membership_id = parse_id(&membership_id)?;
    let row = sqlx::query(
        "SELECT kind, owner_membership_id FROM mailboxes WHERE id = $1 AND workspace_id = $2",
    )
    .bind(mailbox_id.to_string())
    .bind(workspace_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(MailError::NotFound)?;
    let kind: String = row.try_get("kind")?;
    let owner = row
        .try_get::<Option<String>, _>("owner_membership_id")?
        .map(|id| parse_id(&id))
        .transpose()?;
    if kind == "personal" {
        if owner != Some(membership_id) {
            return Err(MailError::NotFound);
        }
        return Ok(Access {
            membership_id,
            personal: true,
            owner,
            may_write: true,
        });
    }
    let manager = actor.can(Permission::MailManage);
    let member: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM mailbox_members WHERE mailbox_id = $1 AND membership_id = $2",
    )
    .bind(mailbox_id.to_string())
    .bind(membership_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?;
    if !manager && member.is_none() {
        return Err(MailError::NotFound);
    }
    Ok(Access {
        membership_id,
        personal: false,
        owner,
        may_write: manager,
    })
}

async fn zone_name(
    transaction: &mut Transaction<'_, Any>,
    access: &Access,
) -> Result<String, MailError> {
    let membership_id = if access.personal {
        access.owner.unwrap_or(access.membership_id)
    } else {
        access.membership_id
    };
    let zone: Option<String> = sqlx::query_scalar(
        "SELECT u.timezone FROM memberships ms JOIN users u ON u.id = ms.user_id WHERE ms.id = $1",
    )
    .bind(membership_id.to_string())
    .fetch_optional(&mut **transaction)
    .await?
    .flatten();
    Ok(zone
        .filter(|name| TimeZone::get(name).is_ok())
        .unwrap_or_else(|| "UTC".to_owned()))
}

fn vacation_row(
    mailbox_id: Id,
    row: &sqlx::any::AnyRow,
    timezone: String,
) -> Result<Vacation, MailError> {
    Ok(Vacation {
        mailbox_id,
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        starts_at: row
            .try_get::<Option<i64>, _>("starts_at")?
            .map(TimestampMillis::from_millis),
        ends_at: row
            .try_get::<Option<i64>, _>("ends_at")?
            .map(TimestampMillis::from_millis),
        subject: row.try_get("subject")?,
        body_html: row.try_get("body_html")?,
        contacts_only: row.try_get::<i64, _>("contacts_only")? != 0,
        timezone,
    })
}

const COLUMNS: &str = "enabled, starts_at, ends_at, subject, body_html, contacts_only";

impl MailRepository {
    /// The vacation settings of a mailbox the caller may see (off, empty when never set).
    pub async fn vacation(
        &self,
        workspace_id: Id,
        user_id: Id,
        mailbox_id: Id,
    ) -> Result<Vacation, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let access = access(&mut transaction, workspace_id, user_id, mailbox_id).await?;
        let timezone = zone_name(&mut transaction, &access).await?;
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM mail_vacations WHERE mailbox_id = $1"
        )))
        .bind(mailbox_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        transaction.commit().await?;
        match row {
            Some(row) => vacation_row(mailbox_id, &row, timezone),
            None => Ok(Vacation {
                mailbox_id,
                enabled: false,
                starts_at: None,
                ends_at: None,
                subject: String::new(),
                body_html: String::new(),
                contacts_only: false,
                timezone,
            }),
        }
    }

    /// Saves the vacation settings: the owner of a personal mailbox, or a member with
    /// `mail.manage` for a shared one (`Forbidden("mail_forbidden")` for another member of it).
    pub async fn put_vacation(
        &self,
        workspace_id: Id,
        user_id: Id,
        mailbox_id: Id,
        change: VacationChange,
        now: TimestampMillis,
    ) -> Result<Vacation, MailError> {
        let subject = change.subject.trim().to_owned();
        if subject.chars().count() > SUBJECT_MAX_CHARS || subject.chars().any(char::is_control) {
            return Err(MailError::Invalid { field: "subject" });
        }
        if change.body_html.len() > BODY_MAX_BYTES {
            return Err(MailError::Invalid { field: "body_html" });
        }
        let body_html = orbit_mail::clean_html(&change.body_html);
        if change.enabled && orbit_mail::html_to_text(&body_html).trim().is_empty() {
            return Err(MailError::Invalid { field: "body_html" });
        }
        let mut transaction = self.database.immediate_transaction().await?;
        let access = access(&mut transaction, workspace_id, user_id, mailbox_id).await?;
        if !access.may_write {
            return Err(MailError::Forbidden("mail_forbidden"));
        }
        let timezone = zone_name(&mut transaction, &access).await?;
        let zone = TimeZone::get(&timezone).unwrap_or(TimeZone::UTC);
        let starts_at = change
            .starts_at
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| moment(text, &zone, "starts_at"))
            .transpose()?;
        let ends_at = change
            .ends_at
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| moment(text, &zone, "ends_at"))
            .transpose()?;
        if let (Some(start), Some(end)) = (starts_at, ends_at)
            && end <= start
        {
            return Err(MailError::Invalid { field: "ends_at" });
        }
        sqlx::query(
            "INSERT INTO mail_vacations (mailbox_id, enabled, starts_at, ends_at, subject, body_html, \
             contacts_only, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8) \
             ON CONFLICT (mailbox_id) DO UPDATE SET enabled = excluded.enabled, \
             starts_at = excluded.starts_at, ends_at = excluded.ends_at, subject = excluded.subject, \
             body_html = excluded.body_html, contacts_only = excluded.contacts_only, \
             updated_at = excluded.updated_at",
        )
        .bind(mailbox_id.to_string())
        .bind(i64::from(change.enabled))
        .bind(starts_at)
        .bind(ends_at)
        .bind(&subject)
        .bind(&body_html)
        .bind(i64::from(change.contacts_only))
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Vacation {
            mailbox_id,
            enabled: change.enabled,
            starts_at: starts_at.map(TimestampMillis::from_millis),
            ends_at: ends_at.map(TimestampMillis::from_millis),
            subject,
            body_html,
            contacts_only: change.contacts_only,
            timezone,
        })
    }

    /// Queues the vacation reply for `message_id`, filed into `thread_id` of `mailbox_id`, when
    /// the rules allow it (see the module docs); `true` when a reply was queued. It runs after
    /// the filing transaction, in its own: a failure here must not undo the ingest, so the caller
    /// logs it.
    pub async fn maybe_vacation_reply(
        &self,
        mailbox_id: Id,
        thread_id: Id,
        message_id: Id,
        now: TimestampMillis,
    ) -> Result<bool, MailError> {
        let pool = self.database.pool();
        let now_ms = now.as_millis();
        // The cheap checks first: a mailbox without a vacation costs one query.
        let Some(settings) = sqlx::query(
            "SELECT v.starts_at, v.ends_at, v.subject, v.body_html, v.contacts_only, \
             mb.workspace_id, mb.kind, mb.owner_membership_id \
             FROM mail_vacations v JOIN mailboxes mb ON mb.id = v.mailbox_id \
             WHERE v.mailbox_id = $1 AND v.enabled = 1 AND mb.status = 'active'",
        )
        .bind(mailbox_id.to_string())
        .fetch_optional(pool)
        .await?
        else {
            return Ok(false);
        };
        let starts_at: Option<i64> = settings.try_get("starts_at")?;
        let ends_at: Option<i64> = settings.try_get("ends_at")?;
        let workspace_id = parse_id(&settings.try_get::<String, _>("workspace_id")?)?;
        let personal = settings.try_get::<String, _>("kind")? == "personal";
        let owner = settings
            .try_get::<Option<String>, _>("owner_membership_id")?
            .map(|id| parse_id(&id))
            .transpose()?;
        let contacts_only = settings.try_get::<i64, _>("contacts_only")? != 0;
        let configured_subject: String = settings.try_get("subject")?;
        let body_html: String = settings.try_get("body_html")?;

        let in_inbox: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mail_threads WHERE id = $1 AND mailbox_id = $2 AND folder = 'inbox'",
        )
        .bind(thread_id.to_string())
        .bind(mailbox_id.to_string())
        .fetch_optional(pool)
        .await?;
        if in_inbox.is_none() {
            return Ok(false);
        }
        let Some(message) = sqlx::query(
            "SELECT m.receipt_id, m.subject, m.to_addrs, m.cc_addrs, r.mail_from, r.auth, r.received_at \
             FROM mail_messages m JOIN mail_receipts r ON r.id = m.receipt_id WHERE m.id = $1",
        )
        .bind(message_id.to_string())
        .fetch_optional(pool)
        .await?
        else {
            return Ok(false);
        };
        // The window is the day the mail arrived, not the day the ingest ran.
        let received_at: i64 = message.try_get("received_at")?;
        if starts_at.is_some_and(|start| received_at < start)
            || ends_at.is_some_and(|end| received_at >= end)
        {
            return Ok(false);
        }
        let mail_from: Option<String> = message.try_get("mail_from")?;
        let Some(sender) = mail_from
            .as_deref()
            .and_then(|from| orbit_mail::normalize_address(from.trim()).ok())
        else {
            return Ok(false);
        };
        // Only an authenticated envelope sender gets a reply (no backscatter to a forged
        // address): SPF passed for its domain or DMARC passed aligned with it, or the message
        // came from a mailbox of this instance (local delivery, `{"local":true}`: signed here,
        // not checked). An `auth` that does not parse is no reason to reply: it means no reply.
        let auth_json: String = message.try_get("auth")?;
        let local = serde_json::from_str::<serde_json::Value>(&auth_json)
            .is_ok_and(|value| value["local"] == serde_json::Value::Bool(true));
        let (dmarc_failed, sender_authenticated) = if local {
            (false, true)
        } else {
            let Ok(auth) = serde_json::from_str::<AuthOutcome>(&auth_json) else {
                return Ok(false);
            };
            (
                matches!(auth.dmarc, DmarcVerdict::Fail { .. }),
                envelope_authenticated(&auth, &sender.domain),
            )
        };
        let sender_text = sender.to_string();
        let own: Vec<(String, String, String, String)> = sqlx::query(
            "SELECT a.id, a.kind, d.id AS domain_id, a.local_part || '@' || d.name AS address \
             FROM mail_addresses a JOIN mail_domains d ON d.id = a.domain_id \
             WHERE a.mailbox_id = $1 ORDER BY a.kind, a.local_part",
        )
        .bind(mailbox_id.to_string())
        .fetch_all(pool)
        .await?
        .iter()
        .map(|row| {
            Ok::<_, MailError>((
                row.try_get("id")?,
                row.try_get("kind")?,
                row.try_get("domain_id")?,
                row.try_get::<String, _>("address")?.to_lowercase(),
            ))
        })
        .collect::<Result<_, _>>()?;
        // The addresses of To and Cc that are the mailbox's, a `+tag` kept, in header order.
        let mut mailbox_addresses: Vec<String> = own
            .iter()
            .map(|(_, _, _, address)| address.clone())
            .collect();
        let mut received_as: Option<usize> = None;
        for column in ["to_addrs", "cc_addrs"] {
            for person in people(&message.try_get::<String, _>(column)?) {
                let Ok(address) = orbit_mail::normalize_address(person.address.trim()) else {
                    continue;
                };
                let base = format!(
                    "{}@{}",
                    orbit_mail::base_local(&address.local),
                    address.domain
                );
                if let Some(index) = own.iter().position(|(_, _, _, own)| *own == base) {
                    mailbox_addresses.push(address.to_string());
                    received_as.get_or_insert(index);
                }
            }
        }
        let chosen = received_as
            .or_else(|| own.iter().position(|(_, kind, _, _)| kind == "primary"))
            .ok_or(MailError::NotFound)?;
        let from_address_id = own[chosen].0.clone();
        let from_domain_id = parse_id(&own[chosen].2)?;

        let sender_is_workspace_mailbox: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM mail_addresses a JOIN mail_domains d ON d.id = a.domain_id \
             WHERE d.workspace_id = $1 AND d.name = $2 AND a.local_part = $3",
        )
        .bind(workspace_id.to_string())
        .bind(&sender.domain)
        .bind(orbit_mail::base_local(&sender.local))
        .fetch_optional(pool)
        .await?;

        let membership_id = match (personal, owner) {
            (true, Some(owner)) => owner,
            (true, None) => return Ok(false),
            (false, _) => {
                let first: Option<String> = sqlx::query_scalar(
                    "SELECT membership_id FROM mailbox_members WHERE mailbox_id = $1 \
                     ORDER BY membership_id LIMIT 1",
                )
                .bind(mailbox_id.to_string())
                .fetch_optional(pool)
                .await?;
                match first {
                    Some(id) => parse_id(&id)?,
                    None => return Ok(false),
                }
            }
        };
        if contacts_only {
            let known: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM mail_contacts c JOIN mailbox_members mm ON mm.membership_id = c.membership_id \
                 WHERE mm.mailbox_id = $1 AND c.address = $2 AND c.hidden = 0 AND c.send_count > 0 LIMIT 1",
            )
            .bind(mailbox_id.to_string())
            .bind(&sender_text)
            .fetch_optional(pool)
            .await?;
            if known.is_none() {
                return Ok(false);
            }
        }
        // The headers need the raw message: the last, the dearest read.
        let raw = self
            .read_raw(parse_id(&message.try_get::<String, _>("receipt_id")?)?)
            .await?;
        let headers = header_pairs(&raw);
        drop(raw);
        // Not ready to send, or the workspace is paused: no reply (nothing to hold it for).
        if !matches!(
            self.send_readiness(from_domain_id, Some(mailbox_id))
                .await?,
            SendReadiness::Ready
        ) {
            return Ok(false);
        }
        let route_id: Option<String> = sqlx::query_scalar(
            "SELECT r.id FROM mail_domains d JOIN mail_send_routes r ON r.id = COALESCE(d.send_route_id, \
             (SELECT s.mail_send_route_id FROM instance_settings s WHERE s.id = 1)) WHERE d.id = $1",
        )
        .bind(from_domain_id.to_string())
        .fetch_optional(pool)
        .await?;

        let original_subject: String = message.try_get("subject")?;
        let subject = if configured_subject.trim().is_empty() {
            let trimmed = original_subject.trim();
            if trimmed
                .get(..3)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re:"))
            {
                trimmed.to_owned()
            } else {
                format!("Re: {trimmed}").trim_end().to_owned()
            }
        } else {
            configured_subject
        };
        let auto = serde_json::to_string(&AutoReply {
            parent_message_id: message_id.to_string(),
            thread_id: thread_id.to_string(),
            from_address_id,
            to: sender_text.clone(),
            subject,
            body_html,
        })
        .map_err(|_| MailError::Invalid {
            field: "auto_reply",
        })?;

        let mut transaction = self.database.immediate_transaction().await?;
        let route = route_id.as_deref().map(parse_id).transpose()?;
        let limited = route_limited(&mut transaction, route).await?;
        if limited && paused(&mut transaction, workspace_id).await? {
            return Ok(false);
        }
        let last: Option<i64> = sqlx::query_scalar(
            "SELECT replied_at FROM mail_vacation_replies WHERE mailbox_id = $1 AND sender = $2",
        )
        .bind(mailbox_id.to_string())
        .bind(&sender_text)
        .fetch_optional(&mut *transaction)
        .await?;
        let input = VacationInput {
            mail_from: mail_from.as_deref(),
            headers: &headers,
            mailbox_addresses: &mailbox_addresses,
            sender_is_workspace_mailbox: sender_is_workspace_mailbox.is_some(),
            in_inbox: true,
            dmarc_failed,
            sender_authenticated,
            replied_within_7_days: last.is_some_and(|at| at > now_ms - REPLY_INTERVAL_MS),
        };
        if !vacation_allowed(&input) {
            return Ok(false);
        }
        // Over a window of the limits: no reply, rather than one held for room.
        if limited
            && self
                .window_allows(
                    &mut transaction,
                    workspace_id,
                    Some(membership_id),
                    1,
                    now,
                    None,
                )
                .await?
                .is_some()
        {
            return Ok(false);
        }
        // The same statement checks the 7 days again: only a new or an old row is replaced.
        let claimed = sqlx::query(
            "INSERT INTO mail_vacation_replies (mailbox_id, sender, replied_at) VALUES ($1, $2, $3) \
             ON CONFLICT (mailbox_id, sender) DO UPDATE SET replied_at = excluded.replied_at \
             WHERE mail_vacation_replies.replied_at <= $4",
        )
        .bind(mailbox_id.to_string())
        .bind(&sender_text)
        .bind(now_ms)
        .bind(now_ms - REPLY_INTERVAL_MS)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if claimed != 1 {
            return Ok(false);
        }
        sqlx::query("DELETE FROM mail_vacation_replies WHERE mailbox_id = $1 AND replied_at <= $2")
            .bind(mailbox_id.to_string())
            .bind(now_ms - REPLY_INTERVAL_MS)
            .execute(&mut *transaction)
            .await?;
        let id = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_outbound (id, workspace_id, membership_id, mailbox_id, draft_id, \
             route_id, status, release_at, scheduled, hold_reason, auto_reply, automatic, \
             created_at, updated_at) \
             VALUES ($1, $2, $3, $4, NULL, $5, 'held', $6, 0, NULL, $7, 1, $6, $6)",
        )
        .bind(id.to_string())
        .bind(workspace_id.to_string())
        .bind(membership_id.to_string())
        .bind(mailbox_id.to_string())
        .bind(route_id)
        .bind(now_ms)
        .bind(auto)
        .execute(&mut *transaction)
        .await?;
        enqueue(&mut transaction, &send_job(id, workspace_id, now)).await?;
        transaction.commit().await?;
        Ok(true)
    }
}
