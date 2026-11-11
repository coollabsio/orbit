//! Mailboxes, their addresses and members (spec §5), the bulk personal mailboxes (§8.2) and
//! the recipient lookup at `RCPT TO` (§6.2).
//!
//! A personal mailbox belongs to one member, who is also its only member. A shared mailbox has
//! any members of the workspace. Members read a mailbox; owners and admins manage all of them.
//! When the owner of a personal mailbox leaves the workspace, the mailbox is suspended in the
//! same transaction ([`suspend_owned_mailboxes`]); its membership row then goes, so nobody reads
//! it, and only the admins see it in the management view, to delete it.
//!
//! `postmaster@<domain>` (RFC 5321 §4.5.1) is routed like any other address: it reaches the
//! mailbox that has it as an address (an admin adds a `postmaster` alias) or the catch-all.

use std::collections::{HashMap, HashSet};

use orbit_domain::{Actor, Permission};
use orbit_platform::{Id, Row, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::any::AnyRow;
use sqlx::{Any, Transaction};
use unicode_normalization::UnicodeNormalization;
use utoipa::ToSchema;

use super::domains::{SUSPENDED_GRACE_MS, manager, parse_id};
use super::{MailError, MailRepository, record_mail_audit};
use crate::repositories::membership;

/// Longest mailbox name.
const NAME_MAX_CHARS: usize = 120;
/// Longest local part (RFC 5321 §4.5.3.1.1).
const LOCAL_MAX_LEN: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = MailboxKind)]
pub enum MailboxKind {
    /// One member's mailbox; that member is its only member.
    Personal,
    /// A team mailbox (`support@`) with any members.
    Shared,
}

impl MailboxKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Shared => "shared",
        }
    }

    fn parse(value: &str) -> Result<Self, MailError> {
        match value {
            "personal" => Ok(Self::Personal),
            "shared" => Ok(Self::Shared),
            _ => Err(MailError::Invalid { field: "kind" }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = MailboxStatus)]
pub enum MailboxStatus {
    Active,
    /// Mail to it is refused (`550 5.2.1`); its members still read what it has.
    Suspended,
}

impl MailboxStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
        }
    }

    fn parse(value: &str) -> Result<Self, MailError> {
        match value {
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            _ => Err(MailError::Invalid { field: "status" }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = MailAddressKind)]
pub enum AddressKind {
    /// The mailbox's own address; one for each mailbox, never removed.
    Primary,
    Alias,
}

impl AddressKind {
    fn parse(value: &str) -> Result<Self, MailError> {
        match value {
            "primary" => Ok(Self::Primary),
            "alias" => Ok(Self::Alias),
            _ => Err(MailError::Invalid { field: "kind" }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailAddress {
    pub id: Id,
    pub domain_id: Id,
    /// Lower case, without a `+tag`.
    pub local_part: String,
    /// The normalized domain name.
    pub domain: String,
    pub kind: AddressKind,
}

impl MailAddress {
    /// `local@domain`.
    #[must_use]
    pub fn address(&self) -> String {
        format!("{}@{}", self.local_part, self.domain)
    }
}

#[derive(Clone, Debug)]
pub struct Mailbox {
    pub id: Id,
    pub workspace_id: Id,
    pub kind: MailboxKind,
    pub name: String,
    /// Personal mailboxes: the owner's membership, `None` once the owner left the workspace.
    pub owner_membership_id: Option<Id>,
    pub status: MailboxStatus,
    /// The primary address first, then the aliases by address.
    pub addresses: Vec<MailAddress>,
    /// Memberships that read the mailbox.
    pub member_ids: Vec<Id>,
    pub created_at: TimestampMillis,
    pub updated_at: TimestampMillis,
}

/// A new mailbox with its primary address `local_part@<domain_id>`.
#[derive(Clone, Debug)]
pub struct NewMailbox {
    pub kind: MailboxKind,
    pub name: String,
    pub domain_id: Id,
    pub local_part: String,
    /// Required for a personal mailbox, refused for a shared one.
    pub owner_membership_id: Option<Id>,
    /// Shared mailboxes: its members. Personal: empty or only the owner.
    pub member_ids: Vec<Id>,
}

/// How **Create a mailbox for each member** names the addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = MailNamePattern)]
pub enum NamePattern {
    /// `jose@`
    First,
    /// `jose.nunez@`
    FirstDotLast,
    /// `jnunez@`
    Flast,
}

/// A proposed personal mailbox for a member without one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = MailboxProposal)]
pub struct Proposal {
    #[schema(value_type = String)]
    pub membership_id: Id,
    /// The member's display name, also the mailbox name.
    pub name: String,
    /// Empty when the name gives nothing usable.
    pub local_part: String,
    /// The address is taken, used by an earlier proposal, or not valid: edit it first.
    pub conflict: bool,
}

#[derive(Clone, Debug)]
pub struct BulkResult {
    pub created: Vec<Mailbox>,
    /// Proposals that were not created; nothing was written for them.
    pub conflicts: Vec<Proposal>,
}

/// What `RCPT TO` does with a recipient (spec §6.2, §9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecipientLookup {
    /// Deliver to the mailbox of `address_id` (for a catch-all: the catch-all mailbox's primary
    /// address).
    Accept {
        address_id: Id,
        mailbox_id: Id,
        workspace_id: Id,
    },
    /// An active test address of the Admin health card; it never reaches a mailbox.
    HealthTest { test_id: Id },
    /// `550 5.1.1`
    UnknownUser,
    /// Not a domain of this server, or one whose claim is not verified yet: `554 5.7.1`.
    NotLocal,
    /// Disabled, or suspended past the grace period: `550 5.7.1`.
    DomainUnavailable,
    /// `550 5.2.1`
    MailboxSuspended,
    /// Not a plain `local@domain` (source routes, `%`/`!` paths, quotes): `501 5.1.3`.
    Invalid,
}

/// Looks up a recipient as sent in `RCPT TO` (between `<>`), in one indexed query:
/// the domain must be verified (or suspended within its grace period); an active test address
/// comes first; then the exact local part, the local part without `+tag`, the catch-all; the
/// mailbox must be active. Nothing is cached, so changes apply to the next command.
pub async fn lookup_recipient<'e, E>(
    executor: E,
    rcpt: &str,
    now: TimestampMillis,
) -> Result<RecipientLookup, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Any>,
{
    let Ok(address) = orbit_mail::normalize_address(rcpt) else {
        return Ok(RecipientLookup::Invalid);
    };
    let base = orbit_mail::base_local(&address.local);
    let Some(row) = sqlx::query(
        "SELECT d.workspace_id, d.status AS domain_status, d.suspended_at, \
         (SELECT t.id FROM mail_health_tests t WHERE t.domain_id = d.id \
          AND t.address_local = $2 AND t.expires_at > $4) AS test_id, \
         a.id AS address_id, m.id AS mailbox_id, m.kind, m.owner_membership_id, \
         m.status AS mailbox_status \
         FROM mail_domains d \
         LEFT JOIN mail_addresses a ON a.id = COALESCE( \
           (SELECT e.id FROM mail_addresses e WHERE e.domain_id = d.id AND e.local_part = $2), \
           (SELECT b.id FROM mail_addresses b WHERE b.domain_id = d.id AND b.local_part = $3), \
           (SELECT c.id FROM mail_addresses c WHERE c.mailbox_id = d.catch_all_mailbox_id \
            AND c.kind = 'primary')) \
         LEFT JOIN mailboxes m ON m.id = a.mailbox_id AND m.workspace_id = d.workspace_id \
         WHERE d.name = $1",
    )
    .bind(&address.domain)
    .bind(&address.local)
    .bind(base)
    .bind(now.as_millis())
    .fetch_optional(executor)
    .await?
    else {
        return Ok(RecipientLookup::NotLocal);
    };
    let status: String = row.try_get("domain_status")?;
    let suspended_at: Option<i64> = row.try_get("suspended_at")?;
    match status.as_str() {
        "verified" => {}
        "suspended" if suspended_at.is_some_and(|at| now.as_millis() < at + SUSPENDED_GRACE_MS) => {
        }
        "pending" => return Ok(RecipientLookup::NotLocal),
        _ => return Ok(RecipientLookup::DomainUnavailable),
    }
    let id = |column: &str| -> Result<Option<Id>, sqlx::Error> {
        let value: Option<String> = row.try_get(column)?;
        // Ids are written by the server; one that does not parse names nothing.
        Ok(value.and_then(|value| value.parse().ok()))
    };
    if let Some(test_id) = id("test_id")? {
        return Ok(RecipientLookup::HealthTest { test_id });
    }
    let (Some(address_id), Some(mailbox_id), Some(workspace_id)) =
        (id("address_id")?, id("mailbox_id")?, id("workspace_id")?)
    else {
        return Ok(RecipientLookup::UnknownUser);
    };
    let kind: String = row.try_get("kind")?;
    let owner: Option<String> = row.try_get("owner_membership_id")?;
    let mailbox_status: String = row.try_get("mailbox_status")?;
    if mailbox_status != "active" || (kind == "personal" && owner.is_none()) {
        return Ok(RecipientLookup::MailboxSuspended);
    }
    Ok(RecipientLookup::Accept {
        address_id,
        mailbox_id,
        workspace_id,
    })
}

/// Suspends the personal mailboxes of `membership_id` before the membership is removed, in the
/// caller's transaction. The membership's removal then sets their owner to `NULL` and takes the
/// `mailbox_members` rows along.
pub(crate) async fn suspend_owned_mailboxes(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Id,
    actor_id: Id,
    request_id: &str,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM mailboxes WHERE workspace_id = $1 AND owner_membership_id = $2",
    )
    .bind(workspace_id.to_string())
    .bind(membership_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    for mailbox_id in ids {
        sqlx::query("UPDATE mailboxes SET status = 'suspended', updated_at = $1 WHERE id = $2")
            .bind(now.as_millis())
            .bind(&mailbox_id)
            .execute(&mut **transaction)
            .await?;
        crate::audit::record(
            transaction,
            workspace_id,
            Some(actor_id),
            "mail.mailbox_suspended",
            crate::audit::AuditOutcome::Success,
            "mailbox",
            mailbox_id.parse().ok(),
            request_id,
            json!({ "reason": "owner_left", "membership_id": membership_id.to_string() }),
            now,
        )
        .await?;
    }
    Ok(())
}

/// `ASCII` letters, digits, `.` and `-` from a name: `José Núñez` → `jose nunez` before the
/// pattern joins the words. Accents go through NFKD; a few letters without a decomposition are
/// spelled out; anything else is dropped.
fn fold_word(word: &str) -> String {
    let mut folded = String::with_capacity(word.len());
    for character in word.nfkd() {
        let spelled = match character {
            'ß' => "ss",
            'æ' | 'Æ' => "ae",
            'œ' | 'Œ' => "oe",
            'ø' | 'Ø' => "o",
            'đ' | 'Đ' | 'ð' | 'Ð' => "d",
            'ł' | 'Ł' => "l",
            'þ' | 'Þ' => "th",
            'ı' => "i",
            _ => "",
        };
        if spelled.is_empty() {
            let lower = character.to_ascii_lowercase();
            if lower.is_ascii_lowercase() || lower.is_ascii_digit() || lower == '.' || lower == '-'
            {
                folded.push(lower);
            }
        } else {
            folded.push_str(spelled);
        }
    }
    folded
}

/// The local part `pattern` makes from a display name; empty when nothing usable is left.
/// Lower case `[a-z0-9.-]`, dots collapsed and trimmed, at most 64 bytes.
#[must_use]
pub fn local_part_for(name: &str, pattern: NamePattern) -> String {
    let words: Vec<String> = name
        .split(|character: char| character.is_whitespace() || character == '_')
        .map(fold_word)
        .filter(|word| {
            word.chars()
                .any(|character| character.is_ascii_alphanumeric())
        })
        .collect();
    let joined = match (words.first(), words.last()) {
        (Some(first), Some(last)) if words.len() > 1 => match pattern {
            NamePattern::First => first.clone(),
            NamePattern::FirstDotLast => format!("{first}.{last}"),
            NamePattern::Flast => {
                let initial = first
                    .chars()
                    .find(char::is_ascii_alphanumeric)
                    .map(String::from)
                    .unwrap_or_default();
                format!("{initial}{last}")
            }
        },
        (Some(only), _) => only.clone(),
        _ => String::new(),
    };
    let mut local = String::with_capacity(joined.len());
    for character in joined.chars() {
        if character == '.' && (local.is_empty() || local.ends_with('.')) {
            continue;
        }
        local.push(character);
    }
    local.truncate(LOCAL_MAX_LEN);
    local
        .trim_matches(|character| character == '.' || character == '-')
        .to_owned()
}

/// A mailbox name: trimmed, 1 to 120 characters, no control characters.
fn clean_name(name: &str) -> Result<String, MailError> {
    let name = name.trim();
    let count = name.chars().count();
    if count == 0 || count > NAME_MAX_CHARS || name.chars().any(char::is_control) {
        return Err(MailError::Invalid { field: "name" });
    }
    Ok(name.to_owned())
}

/// A local part an address can have on `domain`: lower case, a plain dot-atom, no `+` (that
/// starts a tag).
fn clean_local(local: &str, domain: &str) -> Result<String, MailError> {
    let local = local.trim();
    if local.contains('+') {
        return Err(MailError::Invalid {
            field: "local_part",
        });
    }
    orbit_mail::normalize_address(&format!("{local}@{domain}"))
        .map(|address| address.local)
        .map_err(|_| MailError::Invalid {
            field: "local_part",
        })
}

/// Any member of the workspace; `NotFound` for outsiders.
async fn viewer(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    actor_id: Id,
) -> Result<Actor, MailError> {
    membership::actor(&mut **transaction, workspace_id, actor_id)
        .await?
        .ok_or(MailError::NotFound)
}

/// The name of a domain of the workspace (`Invalid { domain_id }` for any other).
async fn domain_name(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    domain_id: Id,
) -> Result<String, MailError> {
    sqlx::query_scalar("SELECT name FROM mail_domains WHERE id = $1 AND workspace_id = $2")
        .bind(domain_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(MailError::Invalid { field: "domain_id" })
}

async fn address_taken(
    transaction: &mut Transaction<'_, Any>,
    domain_id: Id,
    local: &str,
) -> Result<bool, MailError> {
    let found: Option<String> = sqlx::query_scalar(
        "SELECT id FROM mail_addresses WHERE domain_id = $1 AND local_part = $2",
    )
    .bind(domain_id.to_string())
    .bind(local)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(found.is_some())
}

async fn is_workspace_membership(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    membership_id: Id,
) -> Result<bool, MailError> {
    let found: Option<String> =
        // A guest sees only chat, so a mailbox cannot have one.
        sqlx::query_scalar(
            "SELECT id FROM memberships WHERE id = $1 AND workspace_id = $2 AND role <> 'guest'",
        )
            .bind(membership_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut **transaction)
            .await?;
    Ok(found.is_some())
}

async fn has_personal_mailbox(
    transaction: &mut Transaction<'_, Any>,
    membership_id: Id,
) -> Result<bool, MailError> {
    let found: Option<String> =
        sqlx::query_scalar("SELECT id FROM mailboxes WHERE owner_membership_id = $1")
            .bind(membership_id.to_string())
            .fetch_optional(&mut **transaction)
            .await?;
    Ok(found.is_some())
}

/// Which mailboxes [`load_mailboxes`] reads.
pub(super) enum Scope {
    All,
    One(Id),
    /// The mailboxes the user is a member of.
    MemberOf(Id),
}

impl Scope {
    /// The condition on `m` (the mailbox), with `$2` when it needs a value.
    fn filter(&self) -> (&'static str, Option<String>) {
        match self {
            Self::All => ("", None),
            Self::One(id) => (" AND m.id = $2", Some(id.to_string())),
            Self::MemberOf(user_id) => (
                " AND EXISTS (SELECT 1 FROM mailbox_members mm JOIN memberships ms \
                 ON ms.id = mm.membership_id WHERE mm.mailbox_id = m.id AND ms.user_id = $2)",
                Some(user_id.to_string()),
            ),
        }
    }
}

async fn fetch(
    transaction: &mut Transaction<'_, Any>,
    sql: String,
    workspace_id: Id,
    value: Option<&String>,
) -> Result<Vec<AnyRow>, MailError> {
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(workspace_id.to_string());
    if let Some(value) = value {
        query = query.bind(value.clone());
    }
    Ok(query.fetch_all(&mut **transaction).await?)
}

pub(super) async fn load_mailboxes(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    scope: &Scope,
) -> Result<Vec<Mailbox>, MailError> {
    let (filter, value) = scope.filter();
    let rows = fetch(
        transaction,
        format!(
            "SELECT m.id, m.workspace_id, m.kind, m.name, m.owner_membership_id, m.status, \
             m.created_at, m.updated_at FROM mailboxes m WHERE m.workspace_id = $1{filter} \
             ORDER BY lower(m.name), m.id"
        ),
        workspace_id,
        value.as_ref(),
    )
    .await?;
    let address_rows = fetch(
        transaction,
        format!(
            "SELECT a.id, a.mailbox_id, a.domain_id, a.local_part, a.kind, d.name AS domain \
             FROM mail_addresses a JOIN mailboxes m ON m.id = a.mailbox_id \
             JOIN mail_domains d ON d.id = a.domain_id WHERE m.workspace_id = $1{filter} \
             ORDER BY a.kind DESC, d.name, a.local_part"
        ),
        workspace_id,
        value.as_ref(),
    )
    .await?;
    let member_rows = fetch(
        transaction,
        format!(
            "SELECT mm.mailbox_id, mm.membership_id FROM mailbox_members mm \
             JOIN mailboxes m ON m.id = mm.mailbox_id WHERE m.workspace_id = $1{filter} \
             ORDER BY mm.created_at, mm.membership_id"
        ),
        workspace_id,
        value.as_ref(),
    )
    .await?;
    let mut addresses: HashMap<String, Vec<MailAddress>> = HashMap::new();
    for row in &address_rows {
        addresses
            .entry(row.try_get("mailbox_id")?)
            .or_default()
            .push(MailAddress {
                id: parse_id(&row.try_get::<String, _>("id")?)?,
                domain_id: parse_id(&row.try_get::<String, _>("domain_id")?)?,
                local_part: row.try_get("local_part")?,
                domain: row.try_get("domain")?,
                kind: AddressKind::parse(&row.try_get::<String, _>("kind")?)?,
            });
    }
    let mut members: HashMap<String, Vec<Id>> = HashMap::new();
    for row in &member_rows {
        members
            .entry(row.try_get("mailbox_id")?)
            .or_default()
            .push(parse_id(&row.try_get::<String, _>("membership_id")?)?);
    }
    rows.iter()
        .map(|row| {
            let id: String = row.try_get("id")?;
            let owner: Option<String> = row.try_get("owner_membership_id")?;
            Ok(Mailbox {
                workspace_id: parse_id(&row.try_get::<String, _>("workspace_id")?)?,
                kind: MailboxKind::parse(&row.try_get::<String, _>("kind")?)?,
                name: row.try_get("name")?,
                owner_membership_id: owner.as_deref().map(parse_id).transpose()?,
                status: MailboxStatus::parse(&row.try_get::<String, _>("status")?)?,
                addresses: addresses.remove(&id).unwrap_or_default(),
                member_ids: members.remove(&id).unwrap_or_default(),
                created_at: TimestampMillis::from_millis(row.try_get("created_at")?),
                updated_at: TimestampMillis::from_millis(row.try_get("updated_at")?),
                id: parse_id(&id)?,
            })
        })
        .collect()
}

async fn load_one(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    mailbox_id: Id,
) -> Result<Mailbox, MailError> {
    load_mailboxes(transaction, workspace_id, &Scope::One(mailbox_id))
        .await?
        .pop()
        .ok_or(MailError::NotFound)
}

#[allow(clippy::too_many_arguments)]
async fn insert_mailbox(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    kind: MailboxKind,
    name: &str,
    owner: Option<Id>,
    members: &[Id],
    domain_id: Id,
    local: &str,
    now: TimestampMillis,
) -> Result<Id, MailError> {
    let mailbox_id = Id::new_v7();
    sqlx::query(
        "INSERT INTO mailboxes (id, workspace_id, kind, name, owner_membership_id, status, \
         created_at, updated_at) VALUES ($1, $2, $3, $4, $5, 'active', $6, $6)",
    )
    .bind(mailbox_id.to_string())
    .bind(workspace_id.to_string())
    .bind(kind.as_str())
    .bind(name)
    .bind(owner.map(|id| id.to_string()))
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?;
    for member in members {
        sqlx::query(
            "INSERT INTO mailbox_members (mailbox_id, membership_id, created_at) \
             VALUES ($1, $2, $3)",
        )
        .bind(mailbox_id.to_string())
        .bind(member.to_string())
        .bind(now.as_millis())
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query(
        "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
         VALUES ($1, $2, $3, $4, 'primary', $5)",
    )
    .bind(Id::new_v7().to_string())
    .bind(domain_id.to_string())
    .bind(mailbox_id.to_string())
    .bind(local)
    .bind(now.as_millis())
    .execute(&mut **transaction)
    .await?;
    Ok(mailbox_id)
}

/// Members (not guests) of the workspace without a personal mailbox, oldest first, with their names.
async fn members_without_mailbox(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
) -> Result<Vec<(Id, String, String)>, MailError> {
    let rows = sqlx::query(
        "SELECT ms.id, u.display_name, u.email FROM memberships ms \
         JOIN users u ON u.id = ms.user_id WHERE ms.workspace_id = $1 AND ms.role <> 'guest' \
         AND NOT EXISTS (SELECT 1 FROM mailboxes m WHERE m.owner_membership_id = ms.id) \
         ORDER BY ms.created_at, ms.id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    rows.iter()
        .map(|row| {
            Ok((
                parse_id(&row.try_get::<String, _>("id")?)?,
                row.try_get("display_name")?,
                row.try_get("email")?,
            ))
        })
        .collect()
}

/// The proposals for the members without a personal mailbox. `overrides` replace the local
/// part a member gets from `pattern`.
async fn proposals(
    transaction: &mut Transaction<'_, Any>,
    workspace_id: Id,
    domain_id: Id,
    domain: &str,
    pattern: NamePattern,
    overrides: &HashMap<Id, String>,
) -> Result<Vec<Proposal>, MailError> {
    let mut used = HashSet::new();
    let mut proposals = Vec::new();
    for (membership_id, display_name, email) in
        members_without_mailbox(transaction, workspace_id).await?
    {
        let name = clean_name(&display_name)
            .or_else(|_| clean_name(&email))
            .unwrap_or_else(|_| "Mailbox".to_owned());
        let wanted = match overrides.get(&membership_id) {
            Some(local) => local.trim().to_ascii_lowercase(),
            None => {
                let local = local_part_for(&display_name, pattern);
                if local.is_empty() {
                    // A name without Latin letters: the address of the account instead.
                    let email_local = email.split('@').next().unwrap_or_default();
                    local_part_for(orbit_mail::base_local(email_local), pattern)
                } else {
                    local
                }
            }
        };
        let (local_part, valid) = match clean_local(&wanted, domain) {
            Ok(local) => (local, true),
            Err(_) => (wanted, false),
        };
        let conflict = !valid
            || used.contains(&local_part)
            || address_taken(transaction, domain_id, &local_part).await?;
        if valid {
            used.insert(local_part.clone());
        }
        proposals.push(Proposal {
            membership_id,
            name,
            local_part,
            conflict,
        });
    }
    Ok(proposals)
}

impl MailRepository {
    /// [`lookup_recipient`] on the pool.
    pub async fn lookup_recipient(
        &self,
        rcpt: &str,
        now: TimestampMillis,
    ) -> Result<RecipientLookup, MailError> {
        Ok(lookup_recipient(self.database.pool(), rcpt, now).await?)
    }

    /// Owners and admins: every mailbox of the workspace (the management view). Other members:
    /// the mailboxes they are members of.
    pub async fn list_mailboxes(
        &self,
        workspace_id: Id,
        actor_id: Id,
    ) -> Result<Vec<Mailbox>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let actor = viewer(&mut transaction, workspace_id, actor_id).await?;
        let scope = if actor.can(Permission::MailManage) {
            Scope::All
        } else {
            Scope::MemberOf(actor_id)
        };
        let mailboxes = load_mailboxes(&mut transaction, workspace_id, &scope).await?;
        transaction.commit().await?;
        Ok(mailboxes)
    }

    /// A mailbox for its members and for owners and admins; `NotFound` for anyone else.
    pub async fn get_mailbox(
        &self,
        workspace_id: Id,
        actor_id: Id,
        mailbox_id: Id,
    ) -> Result<Mailbox, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        let actor = viewer(&mut transaction, workspace_id, actor_id).await?;
        let scope = if actor.can(Permission::MailManage) {
            Scope::One(mailbox_id)
        } else {
            Scope::MemberOf(actor_id)
        };
        let found = load_mailboxes(&mut transaction, workspace_id, &scope)
            .await?
            .into_iter()
            .find(|mailbox| mailbox.id == mailbox_id)
            .ok_or(MailError::NotFound)?;
        transaction.commit().await?;
        Ok(found)
    }

    pub async fn create_mailbox(
        &self,
        workspace_id: Id,
        actor_id: Id,
        new: NewMailbox,
        request_id: &str,
    ) -> Result<Mailbox, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let name = clean_name(&new.name)?;
        let domain = domain_name(&mut transaction, workspace_id, new.domain_id).await?;
        let local = clean_local(&new.local_part, &domain)?;
        let mut members = Vec::new();
        for member in &new.member_ids {
            if !members.contains(member) {
                members.push(*member);
            }
        }
        let owner = match new.kind {
            MailboxKind::Personal => {
                let owner = new.owner_membership_id.ok_or(MailError::Invalid {
                    field: "owner_membership_id",
                })?;
                if members.iter().any(|member| *member != owner) {
                    return Err(MailError::Invalid {
                        field: "member_ids",
                    });
                }
                members = vec![owner];
                Some(owner)
            }
            MailboxKind::Shared => {
                if new.owner_membership_id.is_some() {
                    return Err(MailError::Invalid {
                        field: "owner_membership_id",
                    });
                }
                None
            }
        };
        for member in &members {
            if !is_workspace_membership(&mut transaction, workspace_id, *member).await? {
                return Err(MailError::Invalid {
                    field: if owner.is_some() {
                        "owner_membership_id"
                    } else {
                        "member_ids"
                    },
                });
            }
        }
        if let Some(owner) = owner
            && has_personal_mailbox(&mut transaction, owner).await?
        {
            return Err(MailError::Conflict("member_has_mailbox"));
        }
        if address_taken(&mut transaction, new.domain_id, &local).await? {
            return Err(MailError::Conflict("address_taken"));
        }
        let mailbox_id = insert_mailbox(
            &mut transaction,
            workspace_id,
            new.kind,
            &name,
            owner,
            &members,
            new.domain_id,
            &local,
            now,
        )
        .await?;
        record_mail_audit(
            &mut transaction,
            workspace_id,
            actor_id,
            "mail.mailbox_created",
            "mailbox",
            mailbox_id,
            request_id,
            json!({ "kind": new.kind.as_str(), "address": format!("{local}@{domain}") }),
            now,
        )
        .await?;
        let created = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        transaction.commit().await?;
        Ok(created)
    }

    /// Renames a mailbox.
    pub async fn update_mailbox(
        &self,
        workspace_id: Id,
        actor_id: Id,
        mailbox_id: Id,
        name: &str,
        request_id: &str,
    ) -> Result<Mailbox, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        let name = clean_name(name)?;
        if name != found.name {
            sqlx::query("UPDATE mailboxes SET name = $1, updated_at = $2 WHERE id = $3")
                .bind(&name)
                .bind(now.as_millis())
                .bind(mailbox_id.to_string())
                .execute(&mut *transaction)
                .await?;
            record_mail_audit(
                &mut transaction,
                workspace_id,
                actor_id,
                "mail.mailbox_updated",
                "mailbox",
                mailbox_id,
                request_id,
                json!({ "name": name }),
                now,
            )
            .await?;
        }
        let updated = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Deletes a mailbox with its addresses, members and threads; a catch-all on it is cleared.
    /// Parsed messages and receipts no thread uses any more go with the `mail.purge` job.
    pub async fn delete_mailbox(
        &self,
        workspace_id: Id,
        actor_id: Id,
        mailbox_id: Id,
        request_id: &str,
    ) -> Result<(), MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        sqlx::query("DELETE FROM mailboxes WHERE id = $1")
            .bind(mailbox_id.to_string())
            .execute(&mut *transaction)
            .await?;
        record_mail_audit(
            &mut transaction,
            workspace_id,
            actor_id,
            "mail.mailbox_deleted",
            "mailbox",
            mailbox_id,
            request_id,
            json!({
                "name": found.name,
                "addresses": found.addresses.iter().map(MailAddress::address).collect::<Vec<_>>(),
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Suspends a mailbox (mail to it is refused) or resumes it. A personal mailbox whose owner
    /// left stays suspended (`Conflict("mailbox_owner_left")`).
    pub async fn set_mailbox_status(
        &self,
        workspace_id: Id,
        actor_id: Id,
        mailbox_id: Id,
        status: MailboxStatus,
        request_id: &str,
    ) -> Result<Mailbox, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        if status == MailboxStatus::Active
            && found.kind == MailboxKind::Personal
            && found.owner_membership_id.is_none()
        {
            return Err(MailError::Conflict("mailbox_owner_left"));
        }
        if status != found.status {
            sqlx::query("UPDATE mailboxes SET status = $1, updated_at = $2 WHERE id = $3")
                .bind(status.as_str())
                .bind(now.as_millis())
                .bind(mailbox_id.to_string())
                .execute(&mut *transaction)
                .await?;
            record_mail_audit(
                &mut transaction,
                workspace_id,
                actor_id,
                match status {
                    MailboxStatus::Active => "mail.mailbox_resumed",
                    MailboxStatus::Suspended => "mail.mailbox_suspended",
                },
                "mailbox",
                mailbox_id,
                request_id,
                json!({}),
                now,
            )
            .await?;
        }
        let updated = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Adds a member to a shared mailbox (`Conflict("mailbox_personal")` for a personal one).
    pub async fn add_member(
        &self,
        workspace_id: Id,
        actor_id: Id,
        mailbox_id: Id,
        membership_id: Id,
        request_id: &str,
    ) -> Result<Mailbox, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        if found.kind == MailboxKind::Personal {
            return Err(MailError::Conflict("mailbox_personal"));
        }
        if !is_workspace_membership(&mut transaction, workspace_id, membership_id).await? {
            return Err(MailError::Invalid {
                field: "membership_id",
            });
        }
        if !found.member_ids.contains(&membership_id) {
            sqlx::query(
                "INSERT INTO mailbox_members (mailbox_id, membership_id, created_at) \
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(mailbox_id.to_string())
            .bind(membership_id.to_string())
            .bind(now.as_millis())
            .execute(&mut *transaction)
            .await?;
            record_mail_audit(
                &mut transaction,
                workspace_id,
                actor_id,
                "mail.mailbox_member_added",
                "mailbox",
                mailbox_id,
                request_id,
                json!({ "membership_id": membership_id.to_string() }),
                now,
            )
            .await?;
        }
        let updated = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Removes a member from a shared mailbox (`Conflict("mailbox_personal")` for a personal
    /// one). Removing someone who is not a member changes nothing.
    pub async fn remove_member(
        &self,
        workspace_id: Id,
        actor_id: Id,
        mailbox_id: Id,
        membership_id: Id,
        request_id: &str,
    ) -> Result<Mailbox, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        if found.kind == MailboxKind::Personal {
            return Err(MailError::Conflict("mailbox_personal"));
        }
        if found.member_ids.contains(&membership_id) {
            sqlx::query("DELETE FROM mailbox_members WHERE mailbox_id = $1 AND membership_id = $2")
                .bind(mailbox_id.to_string())
                .bind(membership_id.to_string())
                .execute(&mut *transaction)
                .await?;
            record_mail_audit(
                &mut transaction,
                workspace_id,
                actor_id,
                "mail.mailbox_member_removed",
                "mailbox",
                mailbox_id,
                request_id,
                json!({ "membership_id": membership_id.to_string() }),
                now,
            )
            .await?;
        }
        let updated = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Adds the alias `local_part@<domain_id>` (any domain of the workspace).
    pub async fn add_alias(
        &self,
        workspace_id: Id,
        actor_id: Id,
        mailbox_id: Id,
        domain_id: Id,
        local_part: &str,
        request_id: &str,
    ) -> Result<Mailbox, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        load_one(&mut transaction, workspace_id, mailbox_id).await?;
        let domain = domain_name(&mut transaction, workspace_id, domain_id).await?;
        let local = clean_local(local_part, &domain)?;
        if address_taken(&mut transaction, domain_id, &local).await? {
            return Err(MailError::Conflict("address_taken"));
        }
        let address_id = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
             VALUES ($1, $2, $3, $4, 'alias', $5)",
        )
        .bind(address_id.to_string())
        .bind(domain_id.to_string())
        .bind(mailbox_id.to_string())
        .bind(&local)
        .bind(now.as_millis())
        .execute(&mut *transaction)
        .await?;
        record_mail_audit(
            &mut transaction,
            workspace_id,
            actor_id,
            "mail.address_added",
            "mail_address",
            address_id,
            request_id,
            json!({ "mailbox_id": mailbox_id.to_string(), "address": format!("{local}@{domain}") }),
            now,
        )
        .await?;
        let updated = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Removes an alias of the mailbox (`Conflict("address_primary")` for its primary address).
    pub async fn remove_address(
        &self,
        workspace_id: Id,
        actor_id: Id,
        mailbox_id: Id,
        address_id: Id,
        request_id: &str,
    ) -> Result<Mailbox, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let found = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        let address = found
            .addresses
            .iter()
            .find(|address| address.id == address_id)
            .ok_or(MailError::NotFound)?;
        if address.kind == AddressKind::Primary {
            return Err(MailError::Conflict("address_primary"));
        }
        sqlx::query("DELETE FROM mail_addresses WHERE id = $1")
            .bind(address_id.to_string())
            .execute(&mut *transaction)
            .await?;
        record_mail_audit(
            &mut transaction,
            workspace_id,
            actor_id,
            "mail.address_removed",
            "mail_address",
            address_id,
            request_id,
            json!({ "mailbox_id": mailbox_id.to_string(), "address": address.address() }),
            now,
        )
        .await?;
        let updated = load_one(&mut transaction, workspace_id, mailbox_id).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// **Create a mailbox for each member**: the addresses `pattern` gives the members without a
    /// personal mailbox on `domain_id`. Nothing is written.
    pub async fn preview_bulk_personal(
        &self,
        workspace_id: Id,
        actor_id: Id,
        domain_id: Id,
        pattern: NamePattern,
    ) -> Result<Vec<Proposal>, MailError> {
        let mut transaction = self.database.pool().begin().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let domain = domain_name(&mut transaction, workspace_id, domain_id).await?;
        let proposals = proposals(
            &mut transaction,
            workspace_id,
            domain_id,
            &domain,
            pattern,
            &HashMap::new(),
        )
        .await?;
        transaction.commit().await?;
        Ok(proposals)
    }

    /// Creates the personal mailboxes of [`Self::preview_bulk_personal`], with the admin's edits
    /// in `overrides` (membership, local part). Proposals that still conflict are returned and
    /// not created; the others are, in one transaction.
    pub async fn bulk_personal(
        &self,
        workspace_id: Id,
        actor_id: Id,
        domain_id: Id,
        pattern: NamePattern,
        overrides: &[(Id, String)],
        request_id: &str,
    ) -> Result<BulkResult, MailError> {
        let now = self.database.database_now().await?;
        let mut transaction = self.database.immediate_transaction().await?;
        manager(&mut transaction, workspace_id, actor_id).await?;
        let domain = domain_name(&mut transaction, workspace_id, domain_id).await?;
        let overrides: HashMap<Id, String> = overrides.iter().cloned().collect();
        let proposals = proposals(
            &mut transaction,
            workspace_id,
            domain_id,
            &domain,
            pattern,
            &overrides,
        )
        .await?;
        let mut created_ids = Vec::new();
        let mut conflicts = Vec::new();
        for proposal in proposals {
            if proposal.conflict {
                conflicts.push(proposal);
                continue;
            }
            let mailbox_id = insert_mailbox(
                &mut transaction,
                workspace_id,
                MailboxKind::Personal,
                &proposal.name,
                Some(proposal.membership_id),
                &[proposal.membership_id],
                domain_id,
                &proposal.local_part,
                now,
            )
            .await?;
            record_mail_audit(
                &mut transaction,
                workspace_id,
                actor_id,
                "mail.mailbox_created",
                "mailbox",
                mailbox_id,
                request_id,
                json!({
                    "kind": "personal",
                    "address": format!("{}@{domain}", proposal.local_part),
                    "bulk": true,
                }),
                now,
            )
            .await?;
            created_ids.push(mailbox_id);
        }
        let mut created = load_mailboxes(&mut transaction, workspace_id, &Scope::All).await?;
        created.retain(|mailbox| created_ids.contains(&mailbox.id));
        transaction.commit().await?;
        Ok(BulkResult { created, conflicts })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_fold_to_ascii_local_parts() {
        assert_eq!(
            local_part_for("José Núñez", NamePattern::FirstDotLast),
            "jose.nunez"
        );
        assert_eq!(local_part_for("José Núñez", NamePattern::First), "jose");
        assert_eq!(local_part_for("José Núñez", NamePattern::Flast), "jnunez");
        assert_eq!(
            local_part_for("  Ann  Marie   O'Brien ", NamePattern::FirstDotLast),
            "ann.obrien"
        );
        assert_eq!(
            local_part_for("Jürgen Groß", NamePattern::FirstDotLast),
            "jurgen.gross"
        );
        assert_eq!(local_part_for("Søren", NamePattern::FirstDotLast), "soren");
        assert_eq!(
            local_part_for("..Dr.. Who.", NamePattern::FirstDotLast),
            "dr.who"
        );
        assert_eq!(local_part_for("王 小明", NamePattern::FirstDotLast), "");
        assert_eq!(local_part_for("", NamePattern::First), "");
        assert_eq!(local_part_for("Ｆｕｌｌ", NamePattern::First), "full");
        assert!(local_part_for(&"a".repeat(100), NamePattern::First).len() <= 64);
    }

    #[test]
    fn local_parts_are_plain_dot_atoms_without_tags() {
        assert_eq!(clean_local(" Ann.Lee ", "example.com").unwrap(), "ann.lee");
        for bad in ["ann+x", "", "a b", ".a", "a..b", "\"a\"", "a%b", "a@b"] {
            assert!(clean_local(bad, "example.com").is_err(), "{bad:?}");
        }
    }
}
