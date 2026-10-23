//! Mail (`/mail`), session-authenticated. Management routes need `mail.manage` (workspace
//! owners and admins); an outsider of the workspace gets `mail_not_found`.

pub(crate) mod domains;
pub(crate) mod labels;
pub(crate) mod mailboxes;

use std::sync::Arc;

use axum::Router;
use axum::extract::Extension;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::routing::{delete, get, post, put};
use orbit_platform::{Id, JobStore, MailMode, RequestId};

use crate::auth_routes::CookieMode;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::mail::{MailError, MailRepository};
use crate::task_routes::{ApiError, authenticate_session, request_id_value, validation};

/// What the setup page needs from the mail configuration.
#[derive(Clone, Debug, Default)]
pub struct MailSettings {
    /// `ORBIT_MAIL_HOSTNAME`: the MX target.
    pub hostname: Option<String>,
    pub mode: MailMode,
}

#[derive(Clone)]
pub struct MailState {
    pub(crate) identity: Arc<IdentityRepository>,
    pub(crate) mail: Arc<MailRepository>,
    pub(crate) jobs: JobStore,
    pub(crate) cookie_mode: CookieMode,
    pub(crate) settings: MailSettings,
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
            jobs: JobStore::new(identity.database().clone()),
            identity,
            mail,
            cookie_mode,
            settings,
        }
    }
}

pub fn mail_router(state: MailState) -> Router {
    Router::new()
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
            "/api/v1/workspaces/{workspace_id}/mail/domains/{domain_id}/catch-all",
            put(domains::set_mail_domain_catch_all),
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
            "/api/v1/workspaces/{workspace_id}/mail/labels",
            get(labels::list_mail_labels).post(labels::create_mail_label),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/mail/labels/{label_id}",
            axum::routing::patch(labels::update_mail_label).delete(labels::delete_mail_label),
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
        Ok(Self {
            workspace_id,
            actor_id: session.user.id,
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
        "mail_domains_disabled" => {
            "This Orbit server does not let workspaces add mail domains. An instance admin can turn it on."
        }
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
        _ => "This action is not possible.",
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
        MailError::Invalid { field } => validation(field, instance, request_id),
        MailError::QuotaExceeded => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "storage_quota_exceeded",
            "Storage quota exceeded",
            "The workspace is over its storage quota.",
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
        }
    }
}
