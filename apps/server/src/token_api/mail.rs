//! Mail (`mail:read`, `mail:write`) as the token's creator uses it in the web app: the same
//! `MailRepository` calls, so mailbox membership, send-as rights, the send policy, quotas and
//! the undo-send hold all apply as they do there. Every call names the creator's user (or
//! workspace membership), never the workspace alone, and a mailbox, thread, draft or message the
//! creator does not read is `NotAvailable`, like a missing one. Service-account tokens get no
//! mail: it would read every mailbox of the workspace owner.
use std::sync::Arc;

use orbit_platform::{Id, StagedUpload, TimestampMillis, UploadService};
use serde::Serialize;
use utoipa::ToSchema;

use super::{Authorized, Result, TokenApi, TokenApiError};
use crate::mail_routes::{detail, not_ready_code, not_ready_detail};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::repositories::instance_settings::InstanceSettingsRepository;
use crate::repositories::mail::{
    AddressKind, CONTACT_LIMIT, ContactKind, Draft, DraftChange, DraftFile, FromAddress, Label,
    MailAddress, MailCounts, MailError, MailRepository, MailThreadDetail, MailThreadPage,
    MailThreadSummary, MailboxKind, MailboxStatus, NewDraft, Outbound, OutboundItem, OutboundView,
    SendReadiness, Snippet, ThreadChange, ThreadListInput, Vacation, VacationChange, parse_query,
};

/// The image types an inline draft file may have (by its bytes), as the web composer.
const INLINE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];
/// The largest attachment whose text `mail_attachment` returns.
const ATTACHMENT_TEXT_BYTES: usize = 100_000;

impl TokenApi {
    #[must_use]
    pub fn with_mail(mut self, mail: Arc<MailRepository>) -> Self {
        self.mail = Some(mail);
        self
    }

    /// The largest draft file upload request, in bytes; `None` without mail.
    #[must_use]
    pub fn mail_upload_limit(&self) -> Option<usize> {
        let mail = self.mail.as_ref()?;
        Some(usize::try_from(mail.uploads().limits().max_request_bytes()).unwrap_or(usize::MAX))
    }
}

/// A mailbox the token's creator reads.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailbox)]
pub struct MailboxView {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    pub kind: MailboxKind,
    /// A suspended mailbox refuses new mail; its threads stay readable.
    pub status: MailboxStatus,
    #[schema(required = true)]
    pub primary_address: Option<String>,
    /// Unread threads in the inbox.
    pub unread: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailLabel)]
pub struct MailLabelView {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    /// `#rrggbb`, lower case.
    pub color: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

impl From<Label> for MailLabelView {
    fn from(label: Label) -> Self {
        Self {
            id: label.id,
            name: label.name,
            color: label.color,
            created_at: label.created_at,
            updated_at: label.updated_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailDraftFile)]
pub struct MailDraftFileView {
    #[schema(value_type = String)]
    pub id: Id,
    pub filename: String,
    pub content_type: String,
    pub size: i64,
    /// Set for an image in the body: use it as `<img src="cid:...">`.
    #[schema(required = true)]
    pub content_id: Option<String>,
    /// Sent as a download link instead of an attachment.
    pub as_link: bool,
    /// The received attachment that a forward copied this file from.
    #[schema(value_type = Option<String>, required = true)]
    pub source_attachment_id: Option<Id>,
}

impl From<DraftFile> for MailDraftFileView {
    fn from(file: DraftFile) -> Self {
        Self {
            id: file.id,
            filename: file.filename,
            content_type: file.content_type,
            size: file.size,
            content_id: file.content_id,
            as_link: file.as_link,
            source_attachment_id: file.source_attachment_id,
        }
    }
}

/// A draft of the token's creator.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailDraft)]
pub struct MailDraftView {
    #[schema(value_type = String)]
    pub id: Id,
    #[schema(value_type = String)]
    pub mailbox_id: Id,
    #[schema(value_type = String)]
    pub from_address_id: Id,
    /// The thread a reply or forward belongs to; `null` for a new message.
    #[schema(value_type = Option<String>, required = true)]
    pub thread_id: Option<Id>,
    #[schema(value_type = Option<String>, required = true)]
    pub parent_message_id: Option<Id>,
    /// `new`, `reply`, `reply_all`, `forward` or `forward_attached`.
    pub mode: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    /// Sanitized HTML.
    pub body_html: String,
    /// Send it back as `version` in the next change.
    pub version: i64,
    pub files: Vec<MailDraftFileView>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

impl From<Draft> for MailDraftView {
    fn from(draft: Draft) -> Self {
        Self {
            id: draft.id,
            mailbox_id: draft.mailbox_id,
            from_address_id: draft.from_address_id,
            thread_id: draft.thread_id,
            parent_message_id: draft.parent_message_id,
            mode: draft.mode.as_str().to_owned(),
            to: draft.to,
            cc: draft.cc,
            bcc: draft.bcc,
            subject: draft.subject,
            body_html: draft.body_html,
            version: draft.version,
            files: draft.files.into_iter().map(Into::into).collect(),
            created_at: draft.created_at,
            updated_at: draft.updated_at,
        }
    }
}

/// A sent message before and while it goes.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailOutbound)]
pub struct MailOutboundView {
    #[schema(value_type = String)]
    pub id: Id,
    /// `held` (in the undo time, scheduled, or over a send limit), `cancelled`, `released` or
    /// `done`.
    pub status: String,
    /// When the message goes: the end of the undo time, or the scheduled time.
    #[schema(value_type = String, format = DateTime)]
    pub release_at: TimestampMillis,
    pub scheduled: bool,
    /// Why a held message waits beyond its time: `rate_limit` or `paused`.
    #[schema(required = true)]
    pub hold_reason: Option<String>,
}

impl From<Outbound> for MailOutboundView {
    fn from(outbound: Outbound) -> Self {
        Self {
            id: outbound.id,
            status: outbound.status,
            release_at: outbound.release_at,
            scheduled: outbound.scheduled,
            hold_reason: outbound.hold_reason,
        }
    }
}

/// A row of the Scheduled or Outbox list.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailOutboundItem)]
pub struct MailOutboundItemView {
    #[serde(flatten)]
    #[schema(inline)]
    pub outbound: MailOutboundView,
    #[schema(value_type = String)]
    pub mailbox_id: Id,
    #[schema(value_type = Option<String>, required = true)]
    pub draft_id: Option<Id>,
    pub subject: String,
    pub to: Vec<String>,
}

impl From<OutboundItem> for MailOutboundItemView {
    fn from(item: OutboundItem) -> Self {
        Self {
            outbound: item.outbound.into(),
            mailbox_id: item.mailbox_id,
            draft_id: item.draft_id,
            subject: item.subject,
            to: item.to,
        }
    }
}

/// A recipient of a draft on the workspace's suppression list: a send skips it.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailSuppressedRecipient)]
pub struct MailSuppressedView {
    pub address: String,
    /// `hard_bounce`, `complaint` or `manual`.
    pub reason: String,
    #[schema(value_type = String, format = DateTime)]
    pub since: TimestampMillis,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailDraftCheck)]
pub struct MailDraftCheck {
    /// The draft's recipients that a send skips. Empty: every recipient gets the message.
    pub suppressed: Vec<MailSuppressedView>,
}

/// A sent draft: the held message, and the recipients the send skips.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailSent)]
pub struct MailSent {
    pub outbound: MailOutboundView,
    /// As `POST /mail/drafts/{draft_id}/check` returned them before the send.
    pub suppressed: Vec<MailSuppressedView>,
}

/// An address the token's creator can send as.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailFromAddress)]
pub struct MailFromAddressView {
    #[schema(value_type = String)]
    pub address_id: Id,
    #[schema(value_type = String)]
    pub mailbox_id: Id,
    pub address: String,
    /// The mailbox name (shared) or the member's name (personal).
    pub display_name: String,
    /// The creator's signature for this address; `null` when none.
    #[schema(required = true)]
    pub signature_html: Option<String>,
    /// `ready` or `not_ready`.
    pub ready: String,
    /// Why it is `not_ready`: `domain_unverified`, `inbound_not_live`, `dkim_inactive`,
    /// `no_route` or `mailbox_suspended`.
    #[schema(required = true)]
    pub not_ready_reason: Option<String>,
    /// The largest message, in bytes as sent, that this address's route takes; `null` without
    /// a route.
    #[schema(required = true)]
    pub max_message_bytes: Option<i64>,
}

impl From<FromAddress> for MailFromAddressView {
    fn from(address: FromAddress) -> Self {
        let (ready, reason) = match address.ready {
            SendReadiness::Ready => ("ready", None),
            SendReadiness::NotReady(reason) => (
                "not_ready",
                serde_json::to_value(reason)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned)),
            ),
        };
        Self {
            address_id: address.address_id,
            mailbox_id: address.mailbox_id,
            address: address.address,
            display_name: address.display_name,
            signature_html: address.signature_html,
            ready: ready.to_owned(),
            not_ready_reason: reason,
            max_message_bytes: address.max_message_bytes,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailSignature)]
pub struct MailSignatureView {
    #[schema(value_type = String)]
    pub address_id: Id,
    /// Sanitized HTML; `null` when there is no signature.
    #[schema(required = true)]
    pub body_html: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailSnippet)]
pub struct MailSnippetView {
    #[schema(value_type = String)]
    pub id: Id,
    pub name: String,
    /// Sanitized HTML.
    pub body_html: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: TimestampMillis,
}

impl From<Snippet> for MailSnippetView {
    fn from(snippet: Snippet) -> Self {
        Self {
            id: snippet.id,
            name: snippet.name,
            body_html: snippet.body_html,
            created_at: snippet.created_at,
            updated_at: snippet.updated_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailSettings)]
pub struct MailSettingsView {
    /// How long a sent message waits before it goes, so that it can be undone: 0, 5, 10, 20 or
    /// 30 seconds.
    pub undo_send_seconds: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailContact)]
pub struct MailContactView {
    pub address: String,
    #[schema(required = true)]
    pub name: Option<String>,
    /// `member` (of the workspace), `contact` (the creator wrote to them) or `seen` (a sender in
    /// the creator's mailboxes).
    pub kind: ContactKind,
}

/// The vacation (out-of-office) reply of a mailbox.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = PublicMailVacation)]
pub struct MailVacationView {
    pub enabled: bool,
    /// The first moment a reply is sent (inclusive); `null` is open.
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub starts_at: Option<TimestampMillis>,
    /// The first moment no reply is sent (exclusive); `null` is open.
    #[schema(value_type = Option<String>, format = DateTime, required = true)]
    pub ends_at: Option<TimestampMillis>,
    /// Empty: `Re:` and the subject of the message.
    pub subject: String,
    /// Sanitized HTML.
    pub body_html: String,
    /// Reply only to people the mailbox's members wrote to.
    pub contacts_only: bool,
    /// The time zone that dates without an offset are read in.
    pub timezone: String,
}

impl From<Vacation> for MailVacationView {
    fn from(vacation: Vacation) -> Self {
        Self {
            enabled: vacation.enabled,
            starts_at: vacation.starts_at,
            ends_at: vacation.ends_at,
            subject: vacation.subject,
            body_html: vacation.body_html,
            contacts_only: vacation.contacts_only,
            timezone: vacation.timezone,
        }
    }
}

/// A received attachment's bytes.
pub struct MailAttachment {
    pub filename: String,
    /// An allowed image type when the bytes are one; otherwise `None`.
    pub image_type: Option<&'static str>,
    pub bytes: Vec<u8>,
}

/// A received attachment as an MCP client reads it.
#[derive(Debug, Serialize)]
pub struct MailAttachmentInfo {
    pub id: Id,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
    /// The file as UTF-8 text, when it is text of at most 100,000 bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Authorized<'_> {
    /// The mail repository for `scope`, after the checks of the web app's mail routes: a
    /// personal token, and mail on for this Orbit server.
    async fn mail(&self, scope: ApiTokenScope) -> Result<&Arc<MailRepository>> {
        let mail = self.content(scope, &self.api.mail)?;
        let enabled = InstanceSettingsRepository::new(mail.database().clone())
            .mail_enabled()
            .await?;
        if !enabled {
            return Err(TokenApiError::Mail(MailError::Forbidden("mail_disabled")));
        }
        Ok(mail)
    }

    /// [`Self::mail`] and the creator's workspace membership, which owns drafts, snippets and
    /// settings.
    async fn mail_member(&self, scope: ApiTokenScope) -> Result<(&Arc<MailRepository>, Id)> {
        let mail = self.mail(scope).await?;
        let membership = mail
            .mail_membership(self.principal.workspace_id, self.principal.actor_id)
            .await
            .map_err(mail_error)?;
        Ok((mail, membership))
    }

    /// The mailboxes the creator is a member of, by name, with their unread inbox threads.
    pub async fn mailboxes(&self) -> Result<Vec<MailboxView>> {
        let mail = self.mail(ApiTokenScope::MailRead).await?;
        let p = self.principal;
        let overview = mail
            .overview(p.workspace_id, p.actor_id)
            .await
            .map_err(mail_error)?;
        Ok(overview
            .mailboxes
            .into_iter()
            .map(|(mailbox, unread)| MailboxView {
                primary_address: mailbox
                    .addresses
                    .iter()
                    .find(|address| address.kind == AddressKind::Primary)
                    .map(MailAddress::address),
                id: mailbox.id,
                name: mailbox.name,
                kind: mailbox.kind,
                status: mailbox.status,
                unread,
            })
            .collect())
    }

    pub async fn mail_threads(&self, input: ThreadListInput) -> Result<MailThreadPage> {
        let mail = self.mail(ApiTokenScope::MailRead).await?;
        let p = self.principal;
        mail.list_threads(p.workspace_id, p.actor_id, input)
            .await
            .map_err(mail_error)
    }

    /// A thread with its messages oldest first: plain-text bodies, attachments' metadata and
    /// delivery states, as the web app's reading pane gets them.
    pub async fn mail_thread(&self, thread_id: Id) -> Result<MailThreadDetail> {
        let mail = self.mail(ApiTokenScope::MailRead).await?;
        let p = self.principal;
        mail.get_thread(p.workspace_id, p.actor_id, thread_id)
            .await
            .map_err(mail_error)
    }

    pub async fn search_mail(
        &self,
        query: &str,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<MailThreadPage> {
        if query.len() > 1_000 {
            return Err(TokenApiError::Invalid("q must be at most 1,000 bytes"));
        }
        let mail = self.mail(ApiTokenScope::MailRead).await?;
        let p = self.principal;
        mail.search_threads(
            p.workspace_id,
            p.actor_id,
            parse_query(query),
            cursor,
            limit,
        )
        .await
        .map_err(mail_error)
    }

    pub async fn mail_counts(&self) -> Result<MailCounts> {
        let mail = self.mail(ApiTokenScope::MailRead).await?;
        let p = self.principal;
        mail.counts(p.workspace_id, p.actor_id)
            .await
            .map_err(mail_error)
    }

    pub async fn update_mail_thread(
        &self,
        thread_id: Id,
        change: &ThreadChange,
    ) -> Result<MailThreadSummary> {
        let mail = self.mail(ApiTokenScope::MailWrite).await?;
        let p = self.principal;
        mail.update_thread(p.workspace_id, p.actor_id, thread_id, change)
            .await
            .map_err(mail_error)
    }

    /// The same change for up to 100 threads; threads the creator does not read are left alone.
    pub async fn update_mail_threads(
        &self,
        thread_ids: &[Id],
        change: &ThreadChange,
    ) -> Result<Vec<MailThreadSummary>> {
        let mail = self.mail(ApiTokenScope::MailWrite).await?;
        let p = self.principal;
        mail.update_threads(p.workspace_id, p.actor_id, thread_ids, change)
            .await
            .map_err(mail_error)
    }

    pub async fn mail_attachment(&self, attachment_id: Id) -> Result<MailAttachment> {
        let mail = self.mail(ApiTokenScope::MailRead).await?;
        let p = self.principal;
        let content = mail
            .attachment_content(p.workspace_id, p.actor_id, attachment_id, None)
            .await
            .map_err(mail_error)?;
        Ok(MailAttachment {
            filename: content
                .part
                .filename
                .unwrap_or_else(|| "attachment".to_owned()),
            image_type: orbit_mail::sniff_image(&content.bytes),
            bytes: content.bytes,
        })
    }

    /// [`Self::mail_attachment`] for a client that reads JSON: the text of a small text file.
    pub async fn mail_attachment_info(&self, attachment_id: Id) -> Result<MailAttachmentInfo> {
        let attachment = self.mail_attachment(attachment_id).await?;
        let size = attachment.bytes.len();
        let text = (attachment.image_type.is_none() && size <= ATTACHMENT_TEXT_BYTES)
            .then(|| String::from_utf8(attachment.bytes).ok())
            .flatten();
        Ok(MailAttachmentInfo {
            id: attachment_id,
            filename: attachment.filename,
            content_type: attachment
                .image_type
                .unwrap_or("application/octet-stream")
                .to_owned(),
            size,
            text,
        })
    }

    /// The creator's drafts, the most recently changed first.
    pub async fn mail_drafts(&self, mailbox_id: Option<Id>) -> Result<Vec<MailDraftView>> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailRead).await?;
        let drafts = mail
            .list_drafts(membership, mailbox_id)
            .await
            .map_err(mail_error)?;
        Ok(drafts.into_iter().map(Into::into).collect())
    }

    pub async fn mail_draft(&self, draft_id: Id) -> Result<MailDraftView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailRead).await?;
        mail.get_draft(membership, draft_id)
            .await
            .map(Into::into)
            .map_err(mail_error)
    }

    /// A new draft, or the creator's draft of the thread when there is one (`false`; a
    /// different `mode` changes its mode).
    pub async fn create_mail_draft(&self, new: NewDraft) -> Result<(MailDraftView, bool)> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.create_draft(membership, self.principal.workspace_id, new)
            .await
            .map(|(draft, created)| (draft.into(), created))
            .map_err(mail_error)
    }

    pub async fn update_mail_draft(
        &self,
        draft_id: Id,
        change: DraftChange,
    ) -> Result<MailDraftView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.update_draft(membership, draft_id, change)
            .await
            .map(Into::into)
            .map_err(mail_error)
    }

    pub async fn delete_mail_draft(&self, draft_id: Id) -> Result<()> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.delete_draft(membership, draft_id)
            .await
            .map_err(mail_error)
    }

    /// Checks that `draft_id` is the creator's before the upload's body is read, and returns
    /// the upload service to stage it with.
    pub async fn mail_draft_uploads(&self, draft_id: Id) -> Result<&UploadService> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.authorize_draft_file(membership, draft_id)
            .await
            .map_err(mail_error)?;
        Ok(mail.uploads())
    }

    /// Adds a staged file to the creator's draft. An `inline` file must be a PNG, JPEG, GIF or
    /// WebP image; it gets a `content_id` for the body.
    pub async fn add_mail_draft_file(
        &self,
        draft_id: Id,
        upload: &StagedUpload,
        inline: bool,
    ) -> Result<MailDraftFileView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        let content_id = if inline {
            if !INLINE_TYPES.contains(&upload.detected_media_type.as_str()) {
                return Err(TokenApiError::Mail(MailError::Unprocessable(
                    "inline_not_image",
                )));
            }
            Some(format!("img-{}@orbit", Id::new_v7()))
        } else {
            None
        };
        mail.add_draft_file(
            membership,
            self.principal.workspace_id,
            draft_id,
            upload,
            content_id.as_deref(),
        )
        .await
        .map(Into::into)
        .map_err(mail_error)
    }

    pub async fn set_mail_draft_file_link(
        &self,
        draft_id: Id,
        file_id: Id,
        as_link: bool,
    ) -> Result<MailDraftFileView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.set_file_as_link(membership, draft_id, file_id, as_link)
            .await
            .map(Into::into)
            .map_err(mail_error)
    }

    pub async fn delete_mail_draft_file(&self, draft_id: Id, file_id: Id) -> Result<()> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.remove_draft_file(membership, draft_id, file_id)
            .await
            .map_err(mail_error)
    }

    /// The draft's recipients that a send would skip (the web app's check before a send).
    pub async fn check_mail_draft(&self, draft_id: Id) -> Result<MailDraftCheck> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailRead).await?;
        self.suppressed(mail, membership, draft_id)
            .await
            .map(|suppressed| MailDraftCheck { suppressed })
    }

    async fn suppressed(
        &self,
        mail: &MailRepository,
        membership: Id,
        draft_id: Id,
    ) -> Result<Vec<MailSuppressedView>> {
        let found = mail
            .draft_suppressions(membership, self.principal.workspace_id, draft_id)
            .await
            .map_err(mail_error)?;
        Ok(found
            .into_iter()
            .map(|entry| MailSuppressedView {
                address: entry.address,
                reason: entry.reason.as_str().to_owned(),
                since: entry.created_at,
            })
            .collect())
    }

    /// Sends the creator's draft as the web app does: the check, then the send, which holds the
    /// message for the creator's undo time (or until `scheduled_at`) under the workspace's send
    /// policy and limits. A second call returns the same message.
    pub async fn send_mail_draft(
        &self,
        draft_id: Id,
        scheduled_at: Option<TimestampMillis>,
    ) -> Result<MailSent> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        let suppressed = self.suppressed(mail, membership, draft_id).await?;
        let now = mail.database().database_now().await?;
        let outbound = mail
            .send_draft(membership, draft_id, scheduled_at, now)
            .await
            .map_err(mail_error)?;
        Ok(MailSent {
            outbound: outbound.into(),
            suppressed,
        })
    }

    /// Undoes a send, or cancels a scheduled one, before it goes; the draft comes back.
    pub async fn cancel_mail_outbound(&self, outbound_id: Id) -> Result<MailDraftView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        let now = mail.database().database_now().await?;
        mail.cancel_outbound(membership, outbound_id, now)
            .await
            .map(Into::into)
            .map_err(mail_error)
    }

    /// The creator's scheduled messages (`scheduled`), or the Outbox (`outbox`): held for the
    /// undo time or a limit, and going.
    pub async fn mail_outbound(
        &self,
        view: &str,
        mailbox_id: Option<Id>,
    ) -> Result<Vec<MailOutboundItemView>> {
        let view = match view {
            "scheduled" => OutboundView::Scheduled,
            "outbox" => OutboundView::Outbox,
            _ => {
                return Err(TokenApiError::Invalid(
                    "view must be `scheduled` or `outbox`",
                ));
            }
        };
        let (mail, membership) = self.mail_member(ApiTokenScope::MailRead).await?;
        let items = mail
            .list_outbound(membership, view, mailbox_id)
            .await
            .map_err(mail_error)?;
        Ok(items.into_iter().map(Into::into).collect())
    }

    pub async fn mail_labels(&self) -> Result<Vec<MailLabelView>> {
        let mail = self.mail(ApiTokenScope::MailRead).await?;
        let p = self.principal;
        let labels = mail
            .list_labels(p.workspace_id, p.actor_id)
            .await
            .map_err(mail_error)?;
        Ok(labels.into_iter().map(Into::into).collect())
    }

    /// Label changes need a creator who manages mail (an owner or admin), as in the web app.
    pub async fn create_mail_label(&self, name: &str, color: &str) -> Result<MailLabelView> {
        let mail = self.mail(ApiTokenScope::MailWrite).await?;
        let p = self.principal;
        mail.create_label(p.workspace_id, p.actor_id, name, color, self.request_id)
            .await
            .map(Into::into)
            .map_err(mail_error)
    }

    pub async fn update_mail_label(
        &self,
        label_id: Id,
        name: Option<&str>,
        color: Option<&str>,
    ) -> Result<MailLabelView> {
        let mail = self.mail(ApiTokenScope::MailWrite).await?;
        let p = self.principal;
        mail.update_label(
            p.workspace_id,
            p.actor_id,
            label_id,
            name,
            color,
            self.request_id,
        )
        .await
        .map(Into::into)
        .map_err(mail_error)
    }

    pub async fn delete_mail_label(&self, label_id: Id) -> Result<()> {
        let mail = self.mail(ApiTokenScope::MailWrite).await?;
        let p = self.principal;
        mail.delete_label(p.workspace_id, p.actor_id, label_id, self.request_id)
            .await
            .map_err(mail_error)
    }

    /// Address suggestions (at most 8): members first, then the people the creator wrote to,
    /// then senders in the creator's mailboxes.
    pub async fn mail_contacts(&self, query: &str) -> Result<Vec<MailContactView>> {
        if query.len() > 320 {
            return Err(TokenApiError::Invalid("q must be at most 320 bytes"));
        }
        let (mail, membership) = self.mail_member(ApiTokenScope::MailRead).await?;
        let hits = mail
            .search_contacts(
                self.principal.workspace_id,
                membership,
                query,
                CONTACT_LIMIT,
                TimestampMillis::now(),
            )
            .await
            .map_err(mail_error)?;
        Ok(hits
            .into_iter()
            .map(|hit| MailContactView {
                address: hit.address,
                name: hit.name,
                kind: hit.kind,
            })
            .collect())
    }

    /// The addresses the creator can send as; with `thread_id`, only those of its mailbox.
    pub async fn mail_from_addresses(
        &self,
        thread_id: Option<Id>,
    ) -> Result<Vec<MailFromAddressView>> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailRead).await?;
        let workspace_id = self.principal.workspace_id;
        let mailbox = match thread_id {
            Some(thread_id) => Some(
                mail.thread_mailbox(workspace_id, membership, thread_id)
                    .await
                    .map_err(mail_error)?,
            ),
            None => None,
        };
        let addresses = mail
            .from_addresses(membership, workspace_id, mailbox)
            .await
            .map_err(mail_error)?;
        Ok(addresses.into_iter().map(Into::into).collect())
    }

    pub async fn mail_signature(&self, address_id: Id) -> Result<MailSignatureView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailRead).await?;
        let body_html = mail
            .signature(membership, self.principal.workspace_id, address_id)
            .await
            .map_err(mail_error)?;
        Ok(MailSignatureView {
            address_id,
            body_html,
        })
    }

    pub async fn put_mail_signature(
        &self,
        address_id: Id,
        body_html: &str,
    ) -> Result<MailSignatureView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        let stored = mail
            .put_signature(
                membership,
                self.principal.workspace_id,
                address_id,
                body_html,
            )
            .await
            .map_err(mail_error)?;
        Ok(MailSignatureView {
            address_id,
            body_html: Some(stored),
        })
    }

    pub async fn delete_mail_signature(&self, address_id: Id) -> Result<()> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.delete_signature(membership, self.principal.workspace_id, address_id)
            .await
            .map_err(mail_error)
    }

    pub async fn mail_snippets(&self) -> Result<Vec<MailSnippetView>> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailRead).await?;
        let snippets = mail.list_snippets(membership).await.map_err(mail_error)?;
        Ok(snippets.into_iter().map(Into::into).collect())
    }

    pub async fn create_mail_snippet(
        &self,
        name: &str,
        body_html: &str,
    ) -> Result<MailSnippetView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.create_snippet(membership, self.principal.workspace_id, name, body_html)
            .await
            .map(Into::into)
            .map_err(mail_error)
    }

    pub async fn update_mail_snippet(
        &self,
        snippet_id: Id,
        name: Option<&str>,
        body_html: Option<&str>,
    ) -> Result<MailSnippetView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.update_snippet(membership, snippet_id, name, body_html)
            .await
            .map(Into::into)
            .map_err(mail_error)
    }

    pub async fn delete_mail_snippet(&self, snippet_id: Id) -> Result<()> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        mail.delete_snippet(membership, snippet_id)
            .await
            .map_err(mail_error)
    }

    pub async fn mail_settings(&self) -> Result<MailSettingsView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailRead).await?;
        let undo_send_seconds = mail.member_settings(membership).await.map_err(mail_error)?;
        Ok(MailSettingsView { undo_send_seconds })
    }

    pub async fn put_mail_settings(&self, undo_send_seconds: i64) -> Result<MailSettingsView> {
        let (mail, membership) = self.mail_member(ApiTokenScope::MailWrite).await?;
        let undo_send_seconds = mail
            .put_member_settings(membership, undo_send_seconds)
            .await
            .map_err(mail_error)?;
        Ok(MailSettingsView { undo_send_seconds })
    }

    /// The owner of a personal mailbox and the members of a shared one read its vacation reply.
    pub async fn mail_vacation(&self, mailbox_id: Id) -> Result<MailVacationView> {
        let mail = self.mail(ApiTokenScope::MailRead).await?;
        let p = self.principal;
        mail.vacation(p.workspace_id, p.actor_id, mailbox_id)
            .await
            .map(Into::into)
            .map_err(mail_error)
    }

    /// The owner of a personal mailbox, or a member who manages mail for a shared one.
    pub async fn put_mail_vacation(
        &self,
        mailbox_id: Id,
        change: VacationChange,
    ) -> Result<MailVacationView> {
        let mail = self.mail(ApiTokenScope::MailWrite).await?;
        let p = self.principal;
        mail.put_vacation(
            p.workspace_id,
            p.actor_id,
            mailbox_id,
            change,
            TimestampMillis::now(),
        )
        .await
        .map(Into::into)
        .map_err(mail_error)
    }
}

/// What a mail refusal tells the client: its HTTP status, a stable code and a sentence, the same
/// as the web app's mail routes.
#[must_use]
pub fn mail_refusal(error: &MailError) -> (u16, &'static str, String) {
    match error {
        MailError::Forbidden(code) => (403, code, detail(code).to_owned()),
        MailError::Conflict(code) => (409, code, detail(code).to_owned()),
        MailError::Unprocessable(code) => (422, code, detail(code).to_owned()),
        MailError::NotReady(reason) => (
            409,
            "send_not_ready",
            format!(
                "{} (reason: {})",
                not_ready_detail(*reason),
                not_ready_code(*reason)
            ),
        ),
        MailError::Invalid { field } => (
            400,
            "invalid_request",
            format!("The value of `{field}` is invalid."),
        ),
        MailError::MessageTooLarge { limit, size } => (
            422,
            "message_too_large",
            format!(
                "{} The message is {size} bytes as sent; the send route takes {limit}.",
                detail("message_too_large")
            ),
        ),
        MailError::DraftSending { outbound_id } => (
            409,
            "draft_sending",
            format!(
                "This draft is being sent (outbound {outbound_id}). Cancel the send to change it."
            ),
        ),
        MailError::QuotaExceeded => (
            413,
            "storage_quota_exceeded",
            "The workspace is over its storage quota.".to_owned(),
        ),
        MailError::RateLimited => (
            429,
            "rate_limited",
            "Too many requests. Try again later.".to_owned(),
        ),
        MailError::NotFound => (
            404,
            "not_found",
            "The resource does not exist or this token cannot use it.".to_owned(),
        ),
        MailError::Crypto | MailError::Upload(_) | MailError::Unavailable(_) => (
            500,
            "internal_error",
            "An unexpected error occurred.".to_owned(),
        ),
    }
}

/// A missing or foreign mail resource is `NotAvailable`, and a failure of the server is
/// `Internal`; every other refusal keeps its stable code.
fn mail_error(error: MailError) -> TokenApiError {
    match error {
        MailError::NotFound => TokenApiError::NotAvailable,
        MailError::Unavailable(error) => TokenApiError::Internal(error.to_string()),
        error @ (MailError::Crypto | MailError::Upload(_)) => {
            TokenApiError::Internal(error.to_string())
        }
        error => TokenApiError::Mail(error),
    }
}
