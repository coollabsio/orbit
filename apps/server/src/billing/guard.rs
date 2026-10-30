//! The read-only guard: in Orbit Cloud, a member's write to `/api/v1/workspaces/{id}[/…]` of a
//! workspace without a subscription that grants access answers 402 `workspace_read_only`. Reads
//! stay open, and so do the writes in [`ALLOWED_WRITES`]. Co-editing sockets open read-only
//! instead ([`ReadOnlyWorkspace`]). During the trial, a member's create in [`LIMITED_CREATES`]
//! answers 402 `trial_limit_reached` once the workspace holds the [`TrialLimit`]. Anyone else's
//! write passes to the handler, which refuses it as on any workspace, so a stranger cannot tell a
//! read-only or trial workspace from a paid one.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use orbit_platform::{Id, Problem, RequestId, TimestampMillis, TrialConfig};

use super::{Billing, TrialLimit, WorkspaceAccess};
use crate::auth_routes::{CookieMode, request_session};
use crate::repositories::billing as repository;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::membership;

const WORKSPACES: &str = "/api/v1/workspaces/";

/// Writes a read-only workspace still accepts: the method and the path after
/// `/api/v1/workspaces/{id}`, with `*` for one segment.
const ALLOWED_WRITES: &[(Method, &str)] = &[
    // Subscribing is how a workspace leaves read-only.
    (Method::POST, "billing/checkout"),
    (Method::POST, "billing/portal"),
    // Leaving: delete or restore the workspace, hand it over, remove members (also yourself).
    (Method::DELETE, ""),
    (Method::POST, "restore"),
    (Method::POST, "transfer-ownership"),
    (Method::DELETE, "members/*"),
    // Shrinking access: revoke invitations, stop an integration or an import.
    (Method::DELETE, "invitations/*"),
    (Method::DELETE, "projects/*/github"),
    (Method::POST, "imports/notion/*/cancel"),
    // Data portability: a workspace export, and API tokens that read everything through the
    // API (revoking one is a security action).
    (Method::POST, "exports"),
    (Method::POST, "api-tokens"),
    (Method::DELETE, "api-tokens/*"),
    // Reads sent as POST, and the reader's own read markers and list preferences.
    (Method::POST, "tasks/query"),
    (Method::POST, "notifications/read-all"),
    (Method::POST, "notifications/*/read"),
    (Method::POST, "chat/read-all"),
    (Method::POST, "chat/read-restore"),
    (Method::POST, "chat/conversations/*/read"),
    (Method::POST, "chat/messages/*/unread"),
    (Method::POST, "chat/threads/*/read"),
    (Method::POST, "pages/*/visit"),
    (Method::PUT, "view-preferences/*"),
];

/// Creates a trial workspace refuses at its limit: the method and the path after
/// `/api/v1/workspaces/{id}`, as in [`ALLOWED_WRITES`].
const LIMITED_CREATES: &[(Method, &str, TrialLimit)] = &[
    (Method::POST, "tasks", TrialLimit::Tasks),
    (Method::POST, "pages", TrialLimit::Pages),
    (Method::POST, "pages/*/duplicate", TrialLimit::Pages),
    (Method::POST, "invitations", TrialLimit::Members),
];

/// Set on a co-editing socket request of a read-only workspace: the socket drops content updates.
#[derive(Clone, Copy, Debug)]
pub struct ReadOnlyWorkspace;

/// What the guard does with a request.
#[derive(Debug, Eq, PartialEq)]
enum Check {
    Pass,
    /// A write that a read-only workspace refuses.
    Write(Id),
    /// A write that a read-only workspace refuses and a trial one refuses at `TrialLimit`.
    Create(Id, TrialLimit),
    /// A co-editing socket, read-only for a read-only workspace.
    Collab(Id),
}

fn classify(method: &Method, path: &str) -> Check {
    let Some(rest) = path.strip_prefix(WORKSPACES) else {
        return Check::Pass;
    };
    let (workspace, rest) = rest.split_once('/').unwrap_or((rest, ""));
    // Axum's `Path` decodes `%30…` into the same id, so the guard has to see the decoded one.
    let workspace = percent_encoding::percent_decode_str(workspace).decode_utf8_lossy();
    let Ok(workspace_id) = workspace.parse::<Id>() else {
        return Check::Pass;
    };
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return if rest.ends_with("/collab") {
            Check::Collab(workspace_id)
        } else {
            Check::Pass
        };
    }
    let rest = rest.trim_end_matches('/');
    if let Some((.., limit)) = LIMITED_CREATES
        .iter()
        .find(|(limited, pattern, _)| limited == method && matches_pattern(pattern, rest))
    {
        return Check::Create(workspace_id, *limit);
    }
    let allowed = ALLOWED_WRITES
        .iter()
        .any(|(allowed, pattern)| allowed == method && matches_pattern(pattern, rest));
    if allowed {
        Check::Pass
    } else {
        Check::Write(workspace_id)
    }
}

fn matches_pattern(pattern: &str, path: &str) -> bool {
    if pattern.is_empty() || path.is_empty() {
        return pattern == path;
    }
    let mut segments = path.split('/');
    pattern.split('/').all(|expected| {
        segments
            .next()
            .is_some_and(|segment| expected == "*" || expected == segment)
    }) && segments.next().is_none()
}

#[derive(Clone)]
struct GuardState {
    billing: Billing,
    identity: Arc<IdentityRepository>,
    cookie_mode: CookieMode,
}

/// Adds the guard to `router` (Orbit Cloud only; a self-hosted router never has it).
pub fn layer(
    router: Router,
    billing: Billing,
    identity: Arc<IdentityRepository>,
    cookie_mode: CookieMode,
) -> Router {
    let state = GuardState {
        billing,
        identity,
        cookie_mode,
    };
    router.layer(middleware::from_fn_with_state(state, guard))
}

async fn guard(State(state): State<GuardState>, mut request: Request, next: Next) -> Response {
    let check = classify(request.method(), request.uri().path());
    let workspace_id = match check {
        Check::Pass => return next.run(request).await,
        Check::Write(id) | Check::Create(id, _) | Check::Collab(id) => id,
    };
    let database = state.billing.database();
    let trial = &state.billing.config().trial;
    let access = match repository::workspace_access(database, workspace_id, trial).await {
        Ok(access) => access,
        Err(error) => {
            tracing::warn!(%error, "workspace access lookup failed");
            return problem(&request, StatusCode::SERVICE_UNAVAILABLE, unavailable());
        }
    };
    let refusal = match (check, access) {
        (Check::Pass, _)
        | (_, WorkspaceAccess::Full)
        | (Check::Write(_) | Check::Collab(_), WorkspaceAccess::Trial) => {
            return next.run(request).await;
        }
        (Check::Collab(_), WorkspaceAccess::ReadOnly) => {
            // The socket route authenticates the caller itself; a stranger gets its 401/404.
            request.extensions_mut().insert(ReadOnlyWorkspace);
            return next.run(request).await;
        }
        (Check::Write(_) | Check::Create(..), WorkspaceAccess::ReadOnly) => Refusal::ReadOnly,
        (Check::Create(_, limit), WorkspaceAccess::Trial) => {
            // Counted before the handler runs, so concurrent creates can pass the limit by a few:
            // a trial limit, not a security boundary.
            match repository::trial_usage(database, workspace_id, limit, TimestampMillis::now())
                .await
            {
                Ok(used) if used < limit.limit(trial) => return next.run(request).await,
                Ok(_) => Refusal::TrialLimit(limit),
                Err(error) => {
                    tracing::warn!(%error, "trial usage lookup failed");
                    return problem(&request, StatusCode::SERVICE_UNAVAILABLE, unavailable());
                }
            }
        }
    };
    match caller_is_member(&state, workspace_id, request.headers()).await {
        Ok(true) => match refusal {
            Refusal::ReadOnly => problem(&request, StatusCode::PAYMENT_REQUIRED, read_only()),
            Refusal::TrialLimit(limit) => {
                let message = trial_limit(limit, trial);
                problem(&request, StatusCode::PAYMENT_REQUIRED, message)
            }
        },
        Ok(false) => next.run(request).await,
        Err(error) => {
            tracing::warn!(%error, "workspace membership lookup failed");
            problem(&request, StatusCode::SERVICE_UNAVAILABLE, unavailable())
        }
    }
}

enum Refusal {
    ReadOnly,
    TrialLimit(TrialLimit),
}

/// Whether the request carries the session of a member of the workspace, as the handlers decide
/// it (session cookie, then membership of a live workspace). Workspace routes take no API tokens.
async fn caller_is_member(
    state: &GuardState,
    workspace_id: Id,
    headers: &HeaderMap,
) -> Result<bool, sqlx::Error> {
    let Some(session) = request_session(&state.identity, state.cookie_mode, headers).await else {
        return Ok(false);
    };
    membership::actor(
        state.billing.database().pool(),
        workspace_id,
        session.user.id,
    )
    .await
    .map(|actor| actor.is_some())
}

struct Message {
    code: &'static str,
    title: &'static str,
    detail: String,
}

/// Also the MCP and co-editing wording, so every surface says the same.
pub const READ_ONLY_DETAIL: &str = "This workspace is read-only because it has no active \
     subscription. Its data stays readable and exportable; the owner can subscribe in Settings → Billing.";

fn read_only() -> Message {
    Message {
        code: "workspace_read_only",
        title: "Workspace is read-only",
        detail: READ_ONLY_DETAIL.to_owned(),
    }
}

fn trial_limit(limit: TrialLimit, trial: &TrialConfig) -> Message {
    Message {
        code: "trial_limit_reached",
        title: "Trial limit reached",
        detail: format!(
            "The trial allows {} {}. Choose a plan to add more.",
            limit.limit(trial),
            limit.noun()
        ),
    }
}

fn unavailable() -> Message {
    Message {
        code: "service_unavailable",
        title: "Service unavailable",
        detail: "The workspace's subscription could not be checked. Try again in a moment."
            .to_owned(),
    }
}

fn problem(request: &Request, status: StatusCode, message: Message) -> Response {
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map_or_else(|| "unknown".to_owned(), ToString::to_string);
    let problem = Problem {
        type_uri: format!("https://docs.orbit.dev/problems/{}", message.code),
        title: message.title.to_owned(),
        status: status.as_u16(),
        code: message.code.to_owned(),
        detail: message.detail,
        instance: request.uri().path().to_owned(),
        request_id,
        errors: None,
        conflict: None,
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/problem+json")
        .body(Body::from(
            serde_json::to_vec(&problem).expect("Problem serialization cannot fail"),
        ))
        .expect("guard Problem response is valid")
}

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use orbit_platform::Id;

    use super::{Check, classify};
    use crate::billing::TrialLimit;

    const WORKSPACE: &str = "01890000-0000-7000-8000-000000000001";

    fn check(method: Method, rest: &str) -> Check {
        classify(&method, &format!("/api/v1/workspaces/{WORKSPACE}{rest}"))
    }

    #[test]
    fn writes_to_a_workspace_are_checked_and_reads_pass() {
        let id: Id = WORKSPACE.parse().unwrap();
        assert_eq!(check(Method::POST, "/tasks/abc/comments"), Check::Write(id));
        assert_eq!(check(Method::PATCH, ""), Check::Write(id));
        assert_eq!(check(Method::DELETE, "/tasks/abc"), Check::Write(id));
        assert_eq!(check(Method::PUT, "/pages/abc/favorite"), Check::Write(id));
        assert_eq!(check(Method::POST, "/tasks/query/extra"), Check::Write(id));
        assert_eq!(check(Method::GET, "/tasks"), Check::Pass);
        assert_eq!(check(Method::HEAD, ""), Check::Pass);
        assert_eq!(check(Method::GET, "/pages/abc/collab"), Check::Collab(id));
        assert_eq!(check(Method::GET, "/tasks/abc/collab"), Check::Collab(id));
    }

    #[test]
    fn creates_of_limited_records_are_checked_against_the_trial_limits() {
        let id: Id = WORKSPACE.parse().unwrap();
        for (method, rest, expected) in [
            (Method::POST, "/tasks", Check::Create(id, TrialLimit::Tasks)),
            (
                Method::POST,
                "/tasks/",
                Check::Create(id, TrialLimit::Tasks),
            ),
            (Method::POST, "/pages", Check::Create(id, TrialLimit::Pages)),
            (
                Method::POST,
                "/pages/p1/duplicate",
                Check::Create(id, TrialLimit::Pages),
            ),
            (
                Method::POST,
                "/invitations",
                Check::Create(id, TrialLimit::Members),
            ),
            (Method::POST, "/pages/p1/move", Check::Write(id)),
            (Method::POST, "/pages/trash/empty", Check::Write(id)),
            (Method::DELETE, "/invitations/i1", Check::Pass),
            (Method::GET, "/tasks", Check::Pass),
        ] {
            assert_eq!(check(method.clone(), rest), expected, "{method} {rest}");
        }
    }

    #[test]
    fn the_allow_list_and_other_paths_pass() {
        for (method, rest) in [
            (Method::POST, "/billing/checkout"),
            (Method::POST, "/billing/portal"),
            (Method::DELETE, ""),
            (Method::DELETE, "/members/m1"),
            (Method::POST, "/tasks/query"),
            (Method::POST, "/notifications/n1/read"),
            (Method::POST, "/chat/conversations/c1/read"),
            (Method::PUT, "/view-preferences/tasks"),
            (Method::DELETE, "/api-tokens/t1"),
        ] {
            assert_eq!(check(method.clone(), rest), Check::Pass, "{method} {rest}");
        }
        assert_eq!(classify(&Method::POST, "/api/v1/workspaces"), Check::Pass);
        assert_eq!(
            classify(&Method::POST, "/api/v1/workspaces/not-an-id/tasks"),
            Check::Pass
        );
        assert_eq!(classify(&Method::POST, "/api/v1/auth/login"), Check::Pass);
        assert_eq!(
            classify(&Method::POST, "/api/v1/billing/stripe/webhook"),
            Check::Pass
        );
    }
}
