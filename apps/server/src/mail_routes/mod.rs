//! Mail (`/mail`), session-authenticated. Management routes need `mail.manage` (workspace
//! owners and admins); an outsider of the workspace gets `mail_not_found`. `/api/v1/admin/mail/*`
//! is for instance admins only.

pub(crate) mod admin;
pub(crate) mod compose;
pub(crate) mod content;
pub(crate) mod domains;
pub(crate) mod drafts;
pub(crate) mod events;
pub(crate) mod inbound;
pub(crate) mod labels;
pub(crate) mod mailboxes;
pub(crate) mod providers;
pub(crate) mod send;
pub(crate) mod send_routes;
pub(crate) mod suppressions;
pub(crate) mod threads;

use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Extension};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::routing::{delete, get, patch, post, put};
use orbit_platform::{Id, MailMode, RequestId};

pub use content::{ViewGrant, ViewKey, ViewPurpose};

use crate::auth_routes::CookieMode;
use crate::mail_delivery::ProviderIntake;
use crate::mail_dns::{DnsLookup, HickoryDns};
use crate::mail_image_proxy::ImageProxy;
use crate::mail_listener::MailListenerStatus;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::instance_settings::InstanceSettingsRepository;
use crate::repositories::mail::{MailError, MailRepository, NotReadyReason};
use crate::task_routes::{ApiError, authenticate_session, request_id_value, validation};

/// What the setup page needs from the mail configuration.
#[derive(Clone, Debug, Default)]
pub struct MailSettings {
    /// `ORBIT_MAIL_ENABLED`: this server runs the SMTP listener.
    pub enabled: bool,
    /// `ORBIT_MAIL_HOSTNAME`: the MX target.
    pub hostname: Option<String>,
    pub mode: MailMode,
}

#[derive(Clone)]
pub struct MailState {
    pub(crate) identity: Arc<IdentityRepository>,
    pub(crate) mail: Arc<MailRepository>,
    pub(crate) cookie_mode: CookieMode,
    pub(crate) settings: MailSettings,
    /// Signs the view, part and image proxy URLs of mail content.
    pub(crate) view_key: ViewKey,
    /// The app's public origin (empty: the request's own origin).
    pub(crate) public_origin: String,
    /// `ORBIT_MAIL_CONTENT_ORIGIN`: where mail content is served, when set.
    pub(crate) content_origin: Option<String>,
    pub(crate) image_proxy: Arc<ImageProxy>,
    /// This server's SMTP listener, for the health card.
    pub(crate) listener: Arc<MailListenerStatus>,
    /// The health card's PTR lookups.
    pub(crate) dns: Arc<dyn DnsLookup>,
    /// Mail from providers' webhooks; `None`: the route answers 503.
    pub(crate) inbound: Option<Arc<ProviderIntake>>,
}

impl MailState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        mail: Arc<MailRepository>,
        cookie_mode: CookieMode,
        settings: MailSettings,
    ) -> Self {
        Self {
            view_key: ViewKey::new(mail.app_key()),
            identity,
            mail,
            cookie_mode,
            settings,
            public_origin: String::new(),
            content_origin: None,
            image_proxy: Arc::new(ImageProxy::new()),
            listener: Arc::default(),
            dns: Arc::new(HickoryDns::new(None)),
            inbound: None,
        }
    }

    /// Receives mail from providers' webhooks.
    #[must_use]
    pub fn with_inbound(mut self, inbound: Arc<ProviderIntake>) -> Self {
        self.inbound = Some(inbound);
        self
    }

    #[must_use]
    pub(crate) fn with_inbound_opt(mut self, inbound: Option<Arc<ProviderIntake>>) -> Self {
        self.inbound = inbound;
        self
    }

    /// The listener the health card reports, and the resolver of its PTR check.
    #[must_use]
    pub fn with_health(
        mut self,
        listener: Arc<MailListenerStatus>,
        dns: Arc<dyn DnsLookup>,
    ) -> Self {
        self.listener = listener;
        self.dns = dns;
        self
    }

    /// The app's public origin (frames mail content) and the origin that serves mail content
    /// (`ORBIT_MAIL_CONTENT_ORIGIN`; the public origin when `None`).
    #[must_use]
    pub fn with_origins(
        mut self,
        public_origin: impl Into<String>,
        content_origin: Option<String>,
    ) -> Self {
        self.public_origin = public_origin.into().trim_end_matches('/').to_owned();
        self.content_origin = content_origin
            .map(|origin| origin.trim_end_matches('/').to_owned())
            .filter(|origin| !origin.is_empty());
        self
    }

    #[must_use]
    pub fn with_image_proxy(mut self, proxy: Arc<ImageProxy>) -> Self {
        self.image_proxy = proxy;
        self
    }

    /// With a content origin, only its host serves mail content (the view document, its parts and images): the app
    /// origin answers 404, so mail stays off the app's origin even if a URL is rewritten.
    pub(crate) fn serves_content(&self, headers: &HeaderMap) -> bool {
        let Some(origin) = &self.content_origin else {
            return true;
        };
        let content_host = origin
            .split_once("://")
            .map_or(origin.as_str(), |(_, host)| host);
        headers
            .get(axum::http::header::HOST)
            .and_then(|host| host.to_str().ok())
            .is_some_and(|host| host.eq_ignore_ascii_case(content_host))
    }

    /// The origin in mail content URLs (empty: root-relative URLs).
    pub(crate) fn content_base(&self) -> &str {
        self.content_origin
            .as_deref()
            .unwrap_or(&self.public_origin)
    }
}

pub fn mail_router(state: MailState) -> Router {
    Router::new()
        // the handler reads the body itself, with a limit for each provider kind
        .route(
            "/api/v1/mail/inbound/{kind}/{provider_id}",
            post(inbound::post_inbound),
        )
        // Mailgun posts the raw message only to a URL that ends in `mime`
        .route(
            "/api/v1/mail/inbound/{kind}/{provider_id}/mime",
            post(inbound::post_inbound),
        )
        // the handler reads the body itself, with a limit for each provider kind
        .route(
            "/api/v1/mail/events/{kind}/{route_id}",
            post(events::post_mail_events),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/domains",
            get(domains::list_mail_domains).post(domains::create_mail_domain),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}",
            get(domains::get_mail_domain).delete(domains::delete_mail_domain),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/check",
            post(domains::check_mail_domain),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/dkim/rotate",
            post(domains::rotate_mail_domain_dkim),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/catch-all",
            put(domains::set_mail_domain_catch_all),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/route",
            put(domains::set_mail_domain_route),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/send-route",
            put(domains::set_mail_domain_send_route),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/send-routes",
            get(send_routes::list_workspace_mail_send_routes)
                .post(send_routes::create_workspace_mail_send_route),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/send-routes/{route_id}",
            patch(send_routes::update_workspace_mail_send_route)
                .delete(send_routes::delete_workspace_mail_send_route),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/send-routes/{route_id}/test",
            post(send_routes::test_workspace_mail_send_route),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/providers/{provider_id}/send-route",
            post(send_routes::create_workspace_mail_provider_send_route),
        )
        .route(
            "/api/v1/admin/mail/send-routes",
            get(send_routes::list_instance_mail_send_routes)
                .post(send_routes::create_instance_mail_send_route),
        )
        .route(
            "/api/v1/admin/mail/send-routes/{route_id}",
            patch(send_routes::update_instance_mail_send_route)
                .delete(send_routes::delete_instance_mail_send_route),
        )
        .route(
            "/api/v1/admin/mail/send-routes/{route_id}/test",
            post(send_routes::test_instance_mail_send_route),
        )
        .route(
            "/api/v1/admin/mail/send-default",
            put(send_routes::set_mail_send_default),
        )
        .route(
            "/api/v1/admin/mail/providers/{provider_id}/send-route",
            post(send_routes::create_instance_mail_provider_send_route),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/providers",
            get(providers::list_workspace_mail_providers)
                .post(providers::create_workspace_mail_provider),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/providers/{provider_id}",
            patch(providers::update_workspace_mail_provider)
                .delete(providers::delete_workspace_mail_provider),
        )
        .route(
            "/api/v1/admin/mail/providers",
            get(providers::list_instance_mail_providers)
                .post(providers::create_instance_mail_provider),
        )
        .route(
            "/api/v1/admin/mail/providers/{provider_id}",
            patch(providers::update_instance_mail_provider)
                .delete(providers::delete_instance_mail_provider),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/enabled",
            put(domains::set_mail_domain_enabled),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes",
            get(mailboxes::list_mailboxes).post(mailboxes::create_mailbox),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes/bulk-personal",
            post(mailboxes::create_personal_mailboxes),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes/bulk-personal/preview",
            post(mailboxes::preview_personal_mailboxes),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}",
            get(mailboxes::get_mailbox)
                .patch(mailboxes::update_mailbox)
                .delete(mailboxes::delete_mailbox),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/vacation",
            get(mailboxes::get_mailbox_vacation).put(mailboxes::put_mailbox_vacation),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/contacts",
            get(compose::search_mail_contacts),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/contacts/{address}",
            delete(compose::delete_mail_contact),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/status",
            put(mailboxes::set_mailbox_status),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/members",
            post(mailboxes::add_mailbox_member),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/members/{membership_id}",
            delete(mailboxes::remove_mailbox_member),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/addresses",
            post(mailboxes::add_mailbox_alias),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/mailboxes/{mailbox_id}/addresses/{address_id}",
            delete(mailboxes::remove_mailbox_address),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/drafts",
            get(drafts::list_mail_drafts).post(drafts::create_mail_draft),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}",
            get(drafts::get_mail_draft)
                .patch(drafts::update_mail_draft)
                .delete(drafts::delete_mail_draft),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/files",
            // Multipart stays streaming; this bounds boundaries, headers and ignored fields too.
            post(drafts::upload_mail_draft_file).layer(DefaultBodyLimit::max(
                usize::try_from(state.mail.uploads().limits().max_request_bytes())
                    .unwrap_or(usize::MAX),
            )),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/send",
            post(send::send_mail_draft),
        )
        .route("/mail/f/{token}", get(content::download_mail_file))
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/threads/{thread_id}/messages/{message_id}/file-links/{link_id}/revoke",
            post(content::revoke_mail_file_link),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/outbound/{outbound_id}/cancel",
            post(send::cancel_mail_outbound),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/outbound",
            get(send::list_mail_outbound),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/outbound/{outbound_id}/recipients/{recipient_id}/retry",
            post(suppressions::retry_mail_recipient),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/suppressions",
            get(suppressions::list_mail_suppressions),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/suppressions/{address}",
            delete(suppressions::delete_mail_suppression),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/files/{file_id}",
            get(drafts::get_mail_draft_file)
                .patch(drafts::update_mail_draft_file)
                .delete(drafts::delete_mail_draft_file),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/drafts/{draft_id}/check",
            post(drafts::check_mail_draft),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/labels",
            get(labels::list_mail_labels).post(labels::create_mail_label),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/labels/{label_id}",
            axum::routing::patch(labels::update_mail_label).delete(labels::delete_mail_label),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/threads",
            get(threads::list_mail_threads),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/threads/bulk",
            post(threads::update_mail_threads),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/threads/{thread_id}",
            get(threads::get_mail_thread).patch(threads::update_mail_thread),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/counts",
            get(threads::get_mail_counts),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/search",
            get(threads::search_mail),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/me",
            get(threads::get_mail_me),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/from-addresses",
            get(compose::list_from_addresses),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/signatures/{address_id}",
            get(compose::get_mail_signature)
                .put(compose::put_mail_signature)
                .delete(compose::delete_mail_signature),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/snippets",
            get(compose::list_mail_snippets).post(compose::create_mail_snippet),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/snippets/{snippet_id}",
            patch(compose::update_mail_snippet).delete(compose::delete_mail_snippet),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/settings/me",
            get(compose::get_mail_settings).put(compose::put_mail_settings),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/messages/{message_id}/view-url",
            post(content::create_mail_view_url),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/messages/{message_id}/raw",
            get(content::download_mail_message),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/attachments/{attachment_id}",
            get(content::download_mail_attachment),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/sender-prefs",
            put(content::set_mail_sender_pref),
        )
        .route(
            "/api/v1/mail/content/{token}",
            get(content::get_mail_content),
        )
        .route(
            "/api/v1/mail/parts/{token}/{attachment_id}",
            get(content::get_mail_part),
        )
        .route("/api/v1/mail/image", get(content::get_mail_image))
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/receipts",
            get(admin::list_failed_mail_receipts),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/receipts/{receipt_id}/retry",
            post(admin::retry_mail_receipt),
        )
        .route(
            "/api/v1/admin/mail",
            get(admin::get_mail_admin).put(admin::set_mail_admin),
        )
        .route(
            "/api/v1/admin/mail/send-policies",
            get(admin::list_mail_send_policies),
        )
        .route(
            "/api/v1/admin/mail/send-policies/{workspace_id}",
            get(admin::get_mail_send_policy_admin).put(admin::set_mail_send_policy),
        )
        .route(
            "/api/v1/admin/mail/send-policies/{workspace_id}/resume",
            post(admin::resume_mail_sending),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/send-policy",
            get(admin::get_mail_send_policy),
        )
        .route(
            "/api/v1/admin/mail/test-address",
            post(admin::create_mail_test_address),
        )
        .route(
            "/api/v1/admin/mail/test-address/{test_id}",
            get(admin::get_mail_test_address),
        )
        .route(
            "/api/v1/admin/mail/domains/{name}/workspace",
            put(domains::move_mail_domain),
        )
        .with_state(state)
}

type RequestIdExtension = Option<Extension<RequestId>>;

/// One authenticated request: who calls, in which workspace, and how to report an error.
struct Call<'a> {
    workspace_id: Id,
    actor_id: Id,
    session_id: Id,
    instance: String,
    request_id: Option<&'a Extension<RequestId>>,
}

impl<'a> Call<'a> {
    async fn enter(
        state: &MailState,
        headers: &HeaderMap,
        uri: &Uri,
        workspace: &str,
        request_id: &'a RequestIdExtension,
    ) -> Result<Self, ApiError> {
        let instance = uri.path().to_owned();
        let request_id = request_id.as_ref();
        let session = authenticate_session(
            &state.identity,
            state.cookie_mode,
            headers,
            &instance,
            request_id,
        )
        .await?;
        let workspace_id = workspace
            .parse()
            .map_err(|_| problem(MailError::NotFound, &instance, request_id))?;
        let enabled = InstanceSettingsRepository::new(state.identity.database().clone())
            .mail_enabled()
            .await
            .map_err(|error| problem(MailError::Unavailable(error), &instance, request_id))?;
        if !enabled {
            return Err(problem(
                MailError::Forbidden("mail_disabled"),
                &instance,
                request_id,
            ));
        }
        Ok(Self {
            workspace_id,
            actor_id: session.user.id,
            session_id: session.id,
            instance,
            request_id,
        })
    }

    /// An id from the path. One that does not parse names nothing.
    fn id(&self, value: &str) -> Result<Id, ApiError> {
        value.parse().map_err(|_| self.problem(MailError::NotFound))
    }

    /// An id from the body.
    fn body_id(&self, value: &str, field: &'static str) -> Result<Id, ApiError> {
        value
            .parse()
            .map_err(|_| self.problem(MailError::Invalid { field }))
    }

    fn request_id(&self) -> &str {
        request_id_value(self.request_id)
    }

    fn problem(&self, error: MailError) -> ApiError {
        problem(error, &self.instance, self.request_id)
    }
}

/// The text of a stable error code.
fn detail(code: &'static str) -> &'static str {
    match code {
        "mail_forbidden" => "Only workspace owners and admins can manage mail.",
        "mail_disabled" => "Mail is off on this Orbit server. An instance admin can turn it on.",
        "domain_claimed" => {
            "This domain, a parent domain or a subdomain of it belongs to another workspace."
        }
        "domain_exists" => "This workspace has this domain already.",
        "domain_has_mailboxes" => "Remove the addresses on this domain first.",
        "address_taken" => "Another mailbox has this address already.",
        "address_primary" => "The primary address of a mailbox cannot be removed.",
        "mailbox_personal" => "A personal mailbox has only its owner as a member.",
        "member_has_mailbox" => "This member has a personal mailbox already.",
        "mailbox_owner_left" => {
            "The owner of this personal mailbox left the workspace. It stays suspended; delete it when it is no longer needed."
        }
        "label_exists" => "A label with this name exists already.",
        "receipt_not_failed" => {
            "Only a failed message, or one that has waited for over an hour, can be retried."
        }
        "domain_not_verified" => "A test message needs a verified domain.",
        "workspace_providers_disabled" => {
            "This Orbit server does not let workspaces add their own mail providers. An instance admin can turn it on."
        }
        "rotation_pending" => {
            "A new DKIM key waits for its DNS record. Publish it, or wait for the check, first."
        }
        "route_is_default" => "Choose another default route, or clear it, first.",
        "route_in_use" => "Use another route on the domains that use this one first.",
        "hostname_required" => {
            "Sending directly needs the mail server's host name (ORBIT_MAIL_HOSTNAME)."
        }
        "private_host_refused" => {
            "This host is on a private network or is not a public address. A workspace route can only use a public mail server."
        }
        "plain_smtp_refused" => {
            "A connection without TLS is only possible for an instance route to a host on a private network."
        }
        "draft_conflict" => "The draft changed after it was read. Refresh and try again.",
        "draft_files_limit" => "A draft can have up to 50 files.",
        "sending_paused" => {
            "Sending is paused for this workspace because of bounces or complaints. The root user can resume it."
        }
        "draft_sending" => {
            "This message is waiting to be sent. Undo or cancel the send to change the draft."
        }
        "send_as_denied" => "You cannot send as this address.",
        "already_sent" => "This message has been sent already, so it cannot be undone.",
        "invalid_address" => "A recipient is not a valid email address.",
        "no_recipients" => "Add at least one recipient.",
        "too_many_recipients" => {
            "This message has more recipients than the workspace may send to at once."
        }
        "message_too_large" => {
            "This message is too large for the send route. Send big files as a link."
        }
        "provider_in_use" => "Route the domains that use this provider elsewhere first.",
        "recipient_not_failed" => "Only a recipient that was not delivered can be retried.",
        "link_forwarded" => "A forwarded file cannot be sent as a link. Remove it or attach it.",
        "link_inline" => "An image in the message body cannot be sent as a link.",
        "link_origin_missing" => {
            "File links need a public address for this Orbit server (ORBIT_PUBLIC_ORIGIN or ORBIT_MAIL_CONTENT_ORIGIN)."
        }
        "inline_not_image" => {
            "Only a PNG, JPEG, GIF or WebP image can go in the message body."
        }
        "reserved_address" => "This address is reserved for bounces. Choose another one.",
        _ => "This action is not possible.",
    }
}

/// The snake_case name of a reason, as in the `not_ready_reason` of a From address.
fn not_ready_code(reason: NotReadyReason) -> &'static str {
    match reason {
        NotReadyReason::DomainUnverified => "domain_unverified",
        NotReadyReason::InboundNotLive => "inbound_not_live",
        NotReadyReason::DkimInactive => "dkim_inactive",
        NotReadyReason::NoRoute => "no_route",
        NotReadyReason::MailboxSuspended => "mailbox_suspended",
    }
}

fn not_ready_detail(reason: NotReadyReason) -> &'static str {
    match reason {
        NotReadyReason::DomainUnverified => "The domain is not verified yet.",
        NotReadyReason::InboundNotLive => {
            "The domain's MX record is not set up yet, so replies could not arrive."
        }
        NotReadyReason::DkimInactive => "The domain's DKIM record is not published yet.",
        NotReadyReason::NoRoute => "The domain has no send route.",
        NotReadyReason::MailboxSuspended => "The mailbox is suspended.",
    }
}

pub(crate) fn problem(
    error: MailError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        MailError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "mail_not_found",
            "Not found",
            "This mail resource is not available.",
            instance,
            request_id,
        ),
        MailError::Forbidden(code) => ApiError::new(
            StatusCode::FORBIDDEN,
            code,
            "Action forbidden",
            detail(code),
            instance,
            request_id,
        ),
        MailError::Conflict(code) => ApiError::new(
            StatusCode::CONFLICT,
            code,
            "Conflict",
            detail(code),
            instance,
            request_id,
        ),
        MailError::NotReady(reason) => ApiError::new(
            StatusCode::CONFLICT,
            "send_not_ready",
            "Conflict",
            not_ready_detail(reason),
            instance,
            request_id,
        )
        .with_reason(not_ready_code(reason)),
        MailError::Invalid { field } => validation(field, instance, request_id),
        MailError::Unprocessable(code) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            code,
            "Unprocessable",
            detail(code),
            instance,
            request_id,
        ),
        MailError::MessageTooLarge { limit, size } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "message_too_large",
            "Unprocessable",
            detail("message_too_large"),
            instance,
            request_id,
        )
        .with_sizes(limit, size),
        MailError::DraftSending { outbound_id } => ApiError::new(
            StatusCode::CONFLICT,
            "draft_sending",
            "Conflict",
            "This reply is being sent. Undo the send to edit it.",
            instance,
            request_id,
        )
        .with_outbound_id(outbound_id.to_string()),
        MailError::QuotaExceeded => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "storage_quota_exceeded",
            "Storage quota exceeded",
            "The workspace is over its storage quota.",
            instance,
            request_id,
        ),
        MailError::RateLimited => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many requests",
            "Too many downloads. Try again later.",
            instance,
            request_id,
        ),
        MailError::Crypto | MailError::Upload(_) | MailError::Unavailable(_) => {
            tracing::error!(error = %error, "mail request");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
                instance,
                request_id,
            )
            .cause(match &error {
                MailError::Unavailable(source) => source.to_string(),
                _ => error.to_string(),
            })
        }
    }
}
