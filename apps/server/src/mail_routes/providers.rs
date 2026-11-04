//! Mail providers (provider intake design §5): the root user's instance providers under
//! `/api/v1/admin/mail/providers`, and a workspace's own under `…/workspaces/{id}/mail/providers`
//! (owners and admins, while the instance switch allows them). Each provider comes with what to set
//! up at the provider; secrets that Orbit makes are shown once, in the create response.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::admin::require_root;
use super::{Call, MailState, RequestIdExtension, problem};
use crate::repositories::instance_settings::InstanceSettingsRepository;
use crate::repositories::mail::{
    MailError, MailProvider, NewProvider, ProviderChange, ProviderKind, ProviderScope,
    ProviderSecrets, ProviderSettings,
};
use crate::task_routes::{ApiError, ApiJson, request_id_value};

type RequestIdRef<'a> = Option<&'a Extension<RequestId>>;

#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailProviderKind {
    Cloudflare,
    Postmark,
    Mailgun,
    Resend,
    Ses,
}

impl From<ProviderKind> for MailProviderKind {
    fn from(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Cloudflare => Self::Cloudflare,
            ProviderKind::Postmark => Self::Postmark,
            ProviderKind::Mailgun => Self::Mailgun,
            ProviderKind::Resend => Self::Resend,
            ProviderKind::Ses => Self::Ses,
        }
    }
}

impl From<MailProviderKind> for ProviderKind {
    fn from(kind: MailProviderKind) -> Self {
        match kind {
            MailProviderKind::Cloudflare => Self::Cloudflare,
            MailProviderKind::Postmark => Self::Postmark,
            MailProviderKind::Mailgun => Self::Mailgun,
            MailProviderKind::Resend => Self::Resend,
            MailProviderKind::Ses => Self::Ses,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MailProviderScope {
    /// Set up by the root user; every workspace may route a domain to it.
    Instance,
    Workspace,
}

#[derive(Serialize, Deserialize, ToSchema, Default)]
pub(crate) struct MailProviderSettings {
    /// The domain's MX must point to one of these; the provider's defaults when empty.
    #[serde(default)]
    mx_hosts: Vec<String>,
    /// SES: the AWS region (required). Mailgun: `eu` for the EU region.
    #[serde(default)]
    region: Option<String>,
    /// SES: the receipt rule's S3 bucket (required) and key prefix.
    #[serde(default)]
    bucket: Option<String>,
    #[serde(default)]
    prefix: Option<String>,
    /// SES: the receipt rule's SNS topic ARN (required); other topics are refused.
    #[serde(default)]
    topic_arn: Option<String>,
}

impl From<MailProviderSettings> for ProviderSettings {
    fn from(settings: MailProviderSettings) -> Self {
        Self {
            mx_hosts: settings.mx_hosts,
            region: settings.region,
            bucket: settings.bucket,
            prefix: settings.prefix,
            topic_arn: settings.topic_arn,
        }
    }
}

/// What to set up at the provider.
#[derive(Serialize, ToSchema)]
pub(crate) struct MailProviderSetup {
    /// The URL the provider sends mail to. Postmark: with its credentials only in the create
    /// response; afterwards without them.
    webhook_url: String,
    /// The steps at the provider, in order.
    steps: Vec<String>,
    /// Cloudflare: the Email Worker to paste.
    worker_script: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailProviderView {
    #[schema(value_type = String)]
    id: Id,
    scope: MailProviderScope,
    kind: MailProviderKind,
    name: String,
    settings: MailProviderSettings,
    /// The names of the secrets that are set (never their values).
    secrets_set: Vec<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    last_received_at: Option<TimestampMillis>,
    /// The last failed delivery, cleared by the next good one.
    last_error: Option<String>,
    /// Domains routed to it: of every workspace for the root user, of this workspace otherwise.
    domain_count: i64,
    /// `null` for an instance provider listed to a workspace.
    setup: Option<MailProviderSetup>,
    /// The secrets Orbit made (Cloudflare, Postmark), only in the response that made them.
    generated_secrets: Option<BTreeMap<String, String>>,
    #[schema(value_type = String, format = DateTime)]
    created_at: TimestampMillis,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MailProviderList {
    providers: Vec<MailProviderView>,
    /// Whether workspaces may add their own providers (the instance switch).
    workspace_providers_enabled: bool,
}

/// The secrets a provider gives (Mailgun, Resend, SES); Orbit makes Cloudflare's and Postmark's.
#[derive(Deserialize, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailProviderSecretsBody {
    /// Mailgun: the HTTP webhook signing key.
    signing_key: Option<String>,
    /// Resend: an API key that can read received emails.
    api_key: Option<String>,
    /// Resend: the webhook's signing secret (`whsec_…`).
    webhook_secret: Option<String>,
    /// SES: an IAM user with `s3:GetObject` and `s3:DeleteObject` on the bucket.
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateMailProviderBody {
    kind: MailProviderKind,
    name: String,
    #[serde(default)]
    secrets: Option<MailProviderSecretsBody>,
    #[serde(default)]
    settings: Option<MailProviderSettings>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateMailProviderBody {
    #[serde(default)]
    name: Option<String>,
    /// Replaces every secret of the provider.
    #[serde(default)]
    secrets: Option<MailProviderSecretsBody>,
    #[serde(default)]
    settings: Option<MailProviderSettings>,
    /// Cloudflare and Postmark: makes new secrets, shown in this response only. The old ones stop
    /// working at once.
    #[serde(default)]
    regenerate_secrets: bool,
}

/// The secrets of `kind` from the body; `None` when a value is missing.
fn secrets(kind: ProviderKind, body: MailProviderSecretsBody) -> Option<ProviderSecrets> {
    let value = |value: Option<String>| value.filter(|value| !value.trim().is_empty());
    Some(match kind {
        ProviderKind::Mailgun => ProviderSecrets::Mailgun {
            signing_key: value(body.signing_key)?,
        },
        ProviderKind::Resend => ProviderSecrets::Resend {
            api_key: value(body.api_key)?,
            webhook_secret: value(body.webhook_secret)?,
        },
        ProviderKind::Ses => ProviderSecrets::Ses {
            access_key_id: value(body.access_key_id)?,
            secret_access_key: value(body.secret_access_key)?,
        },
        // made by Orbit
        ProviderKind::Cloudflare | ProviderKind::Postmark => return None,
    })
}

fn generated(secrets: &ProviderSecrets) -> BTreeMap<String, String> {
    let pairs: Vec<(&str, &str)> = match secrets {
        ProviderSecrets::Cloudflare { signing_secret } => vec![("signing_secret", signing_secret)],
        ProviderSecrets::Postmark {
            basic_user,
            basic_password,
        } => vec![
            ("basic_user", basic_user),
            ("basic_password", basic_password),
        ],
        _ => Vec::new(),
    };
    pairs
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

/// The webhook URL, with Postmark's credentials when they are given.
fn webhook_url(
    state: &MailState,
    provider: &MailProvider,
    postmark: Option<(&str, &str)>,
) -> String {
    let path = format!(
        "/api/v1/mail/inbound/{}/{}{}",
        provider.kind.as_str(),
        provider.id,
        if provider.kind == ProviderKind::Mailgun {
            "/mime"
        } else {
            ""
        }
    );
    match postmark {
        Some((user, password)) => match state.public_origin.split_once("://") {
            Some((scheme, host)) => format!("{scheme}://{user}:{password}@{host}{path}"),
            None => format!("{}{path}", state.public_origin),
        },
        None => format!("{}{path}", state.public_origin),
    }
}

fn steps(provider: &MailProvider) -> Vec<String> {
    let settings = &provider.settings;
    let region = settings.region.as_deref().unwrap_or("<region>");
    let bucket = settings.bucket.as_deref().unwrap_or("<bucket>");
    let steps: Vec<String> = match provider.kind {
        ProviderKind::Cloudflare => vec![
            "In Cloudflare, open the domain, then Email → Email Routing, and turn it on. Cloudflare adds its MX records.".to_owned(),
            "In Workers & Pages, create a Worker, paste the script below and deploy it.".to_owned(),
            "In the Worker's settings, add the variable ORBIT_WEBHOOK_URL with the webhook URL, and the secret ORBIT_SIGNING_SECRET with the signing secret.".to_owned(),
            "In Email Routing → Routing rules, set the catch-all address to Send to a Worker, with this Worker.".to_owned(),
            "In Orbit, route the domain to this provider.".to_owned(),
        ],
        ProviderKind::Postmark => vec![
            "In Postmark, open the server, then the Default Inbound Stream → Settings.".to_owned(),
            "Set the inbound webhook URL to the webhook URL with its credentials, and turn on Include raw email content in JSON payload.".to_owned(),
            "Add the domain as the inbound domain, and point its MX record to inbound.postmarkapp.com.".to_owned(),
            "In Orbit, route the domain to this provider.".to_owned(),
        ],
        ProviderKind::Mailgun => vec![
            "In Mailgun, add the domain and publish the MX records that Mailgun shows.".to_owned(),
            "Create a receiving route: match the recipient .*@<domain>, forward to the webhook URL (it ends in /mime), and stop.".to_owned(),
            "Copy the HTTP webhook signing key into this provider.".to_owned(),
            "In Orbit, route the domain to this provider.".to_owned(),
        ],
        ProviderKind::Resend => vec![
            "In Resend, add the domain, turn on receiving, and publish the MX record that Resend shows. Enter its host here as the MX host.".to_owned(),
            "Add a webhook with the webhook URL and the event email.received. Copy its signing secret (whsec_…) and an API key that can read received emails into this provider.".to_owned(),
            "Resend keeps its own copy of each message; Orbit cannot delete it.".to_owned(),
            "In Orbit, route the domain to this provider.".to_owned(),
        ],
        ProviderKind::Ses => vec![
            format!("In Amazon SES ({region}), verify the domain and point its MX record to inbound-smtp.{region}.amazonaws.com."),
            "Create an SNS topic, enter its ARN here, and add an HTTPS subscription with the webhook URL. Orbit confirms it.".to_owned(),
            format!("Create a receipt rule for the domain with the action Deliver to S3 bucket {bucket} and this SNS topic."),
            format!("Create an IAM user for Orbit with s3:GetObject and s3:DeleteObject on {bucket}, and enter its keys here. Orbit deletes each message from the bucket once it has stored it."),
            "In Orbit, route the domain to this provider.".to_owned(),
        ],
    };
    steps
}

fn view(
    state: &MailState,
    provider: MailProvider,
    own: bool,
    generated_secrets: Option<&ProviderSecrets>,
) -> MailProviderView {
    let postmark = match generated_secrets {
        Some(ProviderSecrets::Postmark {
            basic_user,
            basic_password,
        }) => Some((basic_user.as_str(), basic_password.as_str())),
        _ => None,
    };
    // an instance provider listed to a workspace: its name, kind and MX hosts only, not the
    // instance's account (region, bucket, topic) or its delivery status
    let ProviderSettings {
        mx_hosts,
        region,
        bucket,
        prefix,
        topic_arn,
    } = provider.settings.clone();
    let settings = if own {
        MailProviderSettings {
            mx_hosts,
            region,
            bucket,
            prefix,
            topic_arn,
        }
    } else {
        MailProviderSettings {
            mx_hosts,
            ..MailProviderSettings::default()
        }
    };
    let (last_received_at, last_error) = if own {
        (provider.last_received_at, provider.last_error.clone())
    } else {
        (None, None)
    };
    let setup = own.then(|| MailProviderSetup {
        webhook_url: webhook_url(state, &provider, postmark),
        steps: steps(&provider),
        worker_script: (provider.kind == ProviderKind::Cloudflare)
            .then(|| orbit_mail::inbound::cloudflare::WORKER_SCRIPT.to_owned()),
    });
    MailProviderView {
        id: provider.id,
        scope: if provider.workspace_id.is_some() {
            MailProviderScope::Workspace
        } else {
            MailProviderScope::Instance
        },
        kind: provider.kind.into(),
        name: provider.name,
        settings,
        secrets_set: provider
            .secrets_set
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        last_received_at,
        last_error,
        domain_count: provider.domain_count,
        setup,
        generated_secrets: generated_secrets.map(generated),
        created_at: provider.created_at,
    }
}

async fn workspace_providers_enabled(
    state: &MailState,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<bool, ApiError> {
    InstanceSettingsRepository::new(state.identity.database().clone())
        .mail_workspace_providers_enabled()
        .await
        .map_err(|error| problem(MailError::Unavailable(error), instance, request_id))
}

async fn create(
    state: &MailState,
    scope: ProviderScope,
    actor_id: Id,
    body: CreateMailProviderBody,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<(StatusCode, Json<MailProviderView>), ApiError> {
    let kind = ProviderKind::from(body.kind);
    let created = state
        .mail
        .create_provider(
            scope,
            actor_id,
            NewProvider {
                kind,
                name: body.name,
                secrets: body.secrets.and_then(|given| secrets(kind, given)),
                settings: body.settings.unwrap_or_default().into(),
            },
            request_id_value(request_id),
        )
        .await
        .map_err(|error| problem(error, instance, request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(view(
            state,
            created.provider,
            true,
            created.generated.as_ref(),
        )),
    ))
}

async fn update(
    state: &MailState,
    scope: ProviderScope,
    actor_id: Id,
    provider: &str,
    body: UpdateMailProviderBody,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<Json<MailProviderView>, ApiError> {
    let fail = |error| problem(error, instance, request_id);
    let provider_id: Id = provider.parse().map_err(|_| fail(MailError::NotFound))?;
    // the caller's providers only: another scope's provider is not found (and its kind not used)
    let kind = state
        .mail
        .list_providers(scope, actor_id)
        .await
        .map_err(fail)?
        .into_iter()
        .find(|listed| listed.id == provider_id)
        .ok_or_else(|| fail(MailError::NotFound))?
        .kind;
    let secrets = match body.secrets {
        Some(given) => Some(
            secrets(kind, given).ok_or_else(|| fail(MailError::Invalid { field: "secrets" }))?,
        ),
        None => None,
    };
    let changed = state
        .mail
        .update_provider(
            scope,
            actor_id,
            provider_id,
            ProviderChange {
                name: body.name,
                secrets,
                settings: body.settings.map(Into::into),
                regenerate: body.regenerate_secrets,
            },
            request_id_value(request_id),
        )
        .await
        .map_err(fail)?;
    Ok(Json(view(
        state,
        changed.provider,
        true,
        changed.generated.as_ref(),
    )))
}

async fn delete(
    state: &MailState,
    scope: ProviderScope,
    actor_id: Id,
    provider: &str,
    instance: &str,
    request_id: RequestIdRef<'_>,
) -> Result<StatusCode, ApiError> {
    let fail = |error| problem(error, instance, request_id);
    let provider_id: Id = provider.parse().map_err(|_| fail(MailError::NotFound))?;
    state
        .mail
        .delete_provider(scope, actor_id, provider_id, request_id_value(request_id))
        .await
        .map_err(fail)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The instance providers (root user).
#[utoipa::path(get, path = "/api/v1/admin/mail/providers", responses((status = 200, body = MailProviderList)))]
pub(crate) async fn list_instance_mail_providers(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailProviderList>, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    let providers = state
        .mail
        .list_providers(ProviderScope::Instance, actor_id)
        .await
        .map_err(|error| problem(error, &instance, request_id))?;
    Ok(Json(MailProviderList {
        workspace_providers_enabled: workspace_providers_enabled(&state, &instance, request_id)
            .await?,
        providers: providers
            .into_iter()
            .map(|provider| view(&state, provider, true, None))
            .collect(),
    }))
}

/// Adds an instance provider (root user). Cloudflare's and Postmark's secrets are made by Orbit
/// and shown in this response only.
#[utoipa::path(post, path = "/api/v1/admin/mail/providers", request_body = CreateMailProviderBody, responses((status = 201, body = MailProviderView)))]
pub(crate) async fn create_instance_mail_provider(
    State(state): State<MailState>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailProviderBody>,
) -> Result<(StatusCode, Json<MailProviderView>), ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    create(
        &state,
        ProviderScope::Instance,
        actor_id,
        body,
        &instance,
        request_id,
    )
    .await
}

#[utoipa::path(patch, path = "/api/v1/admin/mail/providers/{provider_id}", params(("provider_id" = String, Path)), request_body = UpdateMailProviderBody, responses((status = 200, body = MailProviderView)))]
pub(crate) async fn update_instance_mail_provider(
    State(state): State<MailState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailProviderBody>,
) -> Result<Json<MailProviderView>, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    update(
        &state,
        ProviderScope::Instance,
        actor_id,
        &provider,
        body,
        &instance,
        request_id,
    )
    .await
}

/// Removes an instance provider that no domain uses.
#[utoipa::path(delete, path = "/api/v1/admin/mail/providers/{provider_id}", params(("provider_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_instance_mail_provider(
    State(state): State<MailState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let request_id = request_id.as_ref();
    let (actor_id, instance) = require_root(&state, &headers, &uri, request_id).await?;
    delete(
        &state,
        ProviderScope::Instance,
        actor_id,
        &provider,
        &instance,
        request_id,
    )
    .await
}

/// The providers a workspace can route domains to: the instance providers (read-only) and its
/// own. Owners and admins.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/mail/providers", params(("workspace_id" = String, Path)), responses((status = 200, body = MailProviderList)))]
pub(crate) async fn list_workspace_mail_providers(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<MailProviderList>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    let providers = state
        .mail
        .providers_for_workspace(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(MailProviderList {
        workspace_providers_enabled: workspace_providers_enabled(
            &state,
            &call.instance,
            call.request_id,
        )
        .await?,
        providers: providers
            .into_iter()
            .map(|provider| {
                let own = provider.workspace_id.is_some();
                view(&state, provider, own, None)
            })
            .collect(),
    }))
}

/// Adds a provider of the workspace, with its own provider account.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/mail/providers", params(("workspace_id" = String, Path)), request_body = CreateMailProviderBody, responses((status = 201, body = MailProviderView)))]
pub(crate) async fn create_workspace_mail_provider(
    State(state): State<MailState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<CreateMailProviderBody>,
) -> Result<(StatusCode, Json<MailProviderView>), ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    create(
        &state,
        ProviderScope::Workspace(call.workspace_id),
        call.actor_id,
        body,
        &call.instance,
        call.request_id,
    )
    .await
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/mail/providers/{provider_id}", params(("workspace_id" = String, Path), ("provider_id" = String, Path)), request_body = UpdateMailProviderBody, responses((status = 200, body = MailProviderView)))]
pub(crate) async fn update_workspace_mail_provider(
    State(state): State<MailState>,
    Path((workspace, provider)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiJson(body): ApiJson<UpdateMailProviderBody>,
) -> Result<Json<MailProviderView>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    update(
        &state,
        ProviderScope::Workspace(call.workspace_id),
        call.actor_id,
        &provider,
        body,
        &call.instance,
        call.request_id,
    )
    .await
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/mail/providers/{provider_id}", params(("workspace_id" = String, Path), ("provider_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn delete_workspace_mail_provider(
    State(state): State<MailState>,
    Path((workspace, provider)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<StatusCode, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    delete(
        &state,
        ProviderScope::Workspace(call.workspace_id),
        call.actor_id,
        &provider,
        &call.instance,
        call.request_id,
    )
    .await
}
