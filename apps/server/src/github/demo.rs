//! Development only: a local stand-in for GitHub with sample pull requests, so the PR review screens show data with
//! no real GitHub App. `ORBIT_DEV_DEMO_GITHUB=1` turns it on: `orbit-server seed` then stores a demo app, repository
//! and member connections, and the development server sends its GitHub calls here. It keeps changes (comments,
//! reviews, merges) in memory until the server stops.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use orbit_platform::{Database, Id, TimestampMillis};
use serde_json::{Value, json};
use url::Url;

use crate::secret_box::encrypt_secret;

/// The environment variable that turns the demo on (development only).
pub const DEMO_FLAG: &str = "ORBIT_DEV_DEMO_GITHUB";
pub const DEMO_REPOSITORY: &str = "acme/orbit";
const APP_ID: i64 = 999_001;
const INSTALLATION_ID: i64 = 999_001;
/// The seeded accounts: (GitHub id, login, access token).
const DEVELOPER: (i64, &str, &str) = (1001, "orbit-dev", "demo-dev");
const MEMBER: (i64, &str, &str) = (1002, "mia-chen", "demo-mia");

#[must_use]
pub fn enabled() -> bool {
    std::env::var(DEMO_FLAG).as_deref() == Ok("1")
}

/// Stores the demo GitHub App, its installation on `acme/orbit`, the project connection, both members' GitHub
/// connections, a task link and a few inbox notices. Safe to run again. `false` when the workspace already has another
/// GitHub App (the demo then stays off for it).
pub async fn seed_demo(
    database: &Database,
    key: &[u8; 32],
    workspace: Id,
    project: Id,
    developer: Id,
    member: Id,
) -> Result<bool, sqlx::Error> {
    let encrypt = |value: &str| {
        encrypt_secret(key, value).map_err(|()| sqlx::Error::Protocol("encrypt".to_owned()))
    };
    let pool = database.pool();
    let workspace_id = workspace.to_string();
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT app_id FROM github_apps WHERE workspace_id = $1")
            .bind(&workspace_id)
            .fetch_optional(pool)
            .await?;
    if existing.is_some_and(|app| app != APP_ID) {
        return Ok(false);
    }
    let now = TimestampMillis::now().as_millis();
    let mut tx = database.immediate_transaction().await?;
    sqlx::query(
        "INSERT INTO github_apps (workspace_id, app_id, slug, private_key_encrypted, webhook_secret_encrypted, created_at, \
         client_id, client_secret_encrypted, callback_origin) VALUES ($1, $2, 'orbit-demo', $3, $4, $5, 'Iv1.demo', $6, NULL) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&workspace_id)
    .bind(APP_ID)
    .bind(encrypt("demo-private-key")?)
    .bind(encrypt("demo-webhook-secret")?)
    .bind(now)
    .bind(encrypt("demo-client-secret")?)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO github_installations (installation_id, workspace_id, account_login) VALUES ($1, $2, 'acme') ON CONFLICT DO NOTHING")
        .bind(INSTALLATION_ID)
        .bind(&workspace_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO github_installation_repositories (installation_id, repository) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(INSTALLATION_ID)
        .bind(DEMO_REPOSITORY)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO github_project_connections (project_id, workspace_id, installation_id, repository, label) \
         VALUES ($1, $2, $3, $4, 'Orbit') ON CONFLICT DO NOTHING",
    )
    .bind(project.to_string())
    .bind(&workspace_id)
    .bind(INSTALLATION_ID)
    .bind(DEMO_REPOSITORY)
    .execute(&mut *tx)
    .await?;
    for (user, (github_id, login, token)) in [(developer, DEVELOPER), (member, MEMBER)] {
        sqlx::query(
            "INSERT INTO github_user_connections (workspace_id, user_id, github_user_id, login, access_token_encrypted, \
             access_expires_at, refresh_token_encrypted, refresh_expires_at, broken, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, NULL, NULL, NULL, 0, $6, $7) ON CONFLICT DO NOTHING",
        )
        .bind(&workspace_id)
        .bind(user.to_string())
        .bind(github_id)
        .bind(login)
        .bind(encrypt(token)?)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    let task: Option<String> = sqlx::query_scalar(
        "SELECT id FROM tasks WHERE project_id = $1 AND deleted_at IS NULL ORDER BY created_at LIMIT 1",
    )
    .bind(project.to_string())
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(task) = task {
        sqlx::query(
            "INSERT INTO github_pull_links (workspace_id, repository, pull_number, task_id, title, url, state, updated_at) \
             VALUES ($1, $2, 482, $3, $4, $5, 'open', $6) ON CONFLICT DO NOTHING",
        )
        .bind(&workspace_id)
        .bind(DEMO_REPOSITORY)
        .bind(task)
        .bind(PULL_482_TITLE)
        .bind(format!("https://github.com/{DEMO_REPOSITORY}/pull/482"))
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    let notices: [(&str, i64, Option<&str>, &str, i64); 3] = [
        (
            "github_review_requested",
            482,
            None,
            "github:demo:rr:482",
            25,
        ),
        (
            "github_review",
            477,
            Some("approved"),
            "github:demo:review:477",
            90,
        ),
        (
            "github_merged",
            470,
            None,
            "github:demo:merged:470",
            26 * 60,
        ),
    ];
    for (kind, number, review_state, dedupe, minutes_ago) in notices {
        sqlx::query(
            "INSERT INTO notifications (id, workspace_id, recipient_user_id, actor_user_id, kind, dedupe_key, created_at, \
             github_repository, github_pull_number, github_actor_login, github_review_state) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(Id::new_v7().to_string())
        .bind(&workspace_id)
        .bind(developer.to_string())
        .bind(member.to_string())
        .bind(kind)
        .bind(dedupe)
        .bind(now - minutes_ago * 60_000)
        .bind(DEMO_REPOSITORY)
        .bind(number)
        .bind(MEMBER.1)
        .bind(review_state)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// Starts the demo GitHub on a free local port; the base URL for [`super::GithubEndpoints::local`].
pub async fn start_demo_github() -> Url {
    let state = Arc::new(Mutex::new(Demo::sample()));
    let router = Router::new().fallback(handle).with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a local port");
    let address = listener.local_addr().expect("local address");
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            tracing::warn!(%error, "demo GitHub stopped");
        }
    });
    Url::parse(&format!("http://{address}/")).expect("valid URL")
}

// ---- the sample data ----

const PULL_482_TITLE: &str = "Rate-limit GitHub webhook deliveries per installation";

struct DemoFile {
    path: String,
    previous_path: Option<String>,
    status: &'static str,
    old: Option<String>,
    new: Option<String>,
    patch: Option<String>,
    additions: i64,
    deletions: i64,
}

impl DemoFile {
    /// A file from marked lines: `+` added, `-` removed, a space for both sides.
    fn changed(path: &str, marked: &str) -> Self {
        let lines: Vec<(char, &str)> = marked
            .lines()
            .map(|line| {
                let mut chars = line.chars();
                let kind = chars.next().unwrap_or(' ');
                (kind, chars.as_str())
            })
            .collect();
        let side = |keep: char| {
            let text: Vec<&str> = lines
                .iter()
                .filter(|(kind, _)| *kind == ' ' || *kind == keep)
                .map(|(_, text)| *text)
                .collect();
            (!text.is_empty()).then(|| text.join("\n") + "\n")
        };
        let (old, new) = (side('-'), side('+'));
        let status = match (&old, &new) {
            (None, _) => "added",
            (_, None) => "removed",
            _ => "modified",
        };
        Self {
            path: path.to_owned(),
            previous_path: None,
            status,
            additions: lines.iter().filter(|(kind, _)| *kind == '+').count() as i64,
            deletions: lines.iter().filter(|(kind, _)| *kind == '-').count() as i64,
            patch: Some(patch(&lines)),
            old,
            new,
        }
    }

    fn renamed(mut self, from: &str) -> Self {
        self.previous_path = Some(from.to_owned());
        self.status = "renamed";
        self
    }

    /// A file GitHub sends no patch for (too large).
    fn large(path: &str, additions: i64, deletions: i64) -> Self {
        Self {
            path: path.to_owned(),
            previous_path: None,
            status: "modified",
            old: None,
            new: None,
            patch: None,
            additions,
            deletions,
        }
    }

    /// The new-file line that contains `needle` (1-based).
    fn line_of(&self, needle: &str) -> i64 {
        let new = self.new.as_deref().unwrap_or_default();
        new.lines()
            .position(|line| line.contains(needle))
            .map_or(1, |index| index as i64 + 1)
    }
}

/// A unified diff (GitHub's `patch`): hunks with three lines of context.
fn patch(lines: &[(char, &str)]) -> String {
    const CONTEXT: usize = 3;
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (index, _) in lines
        .iter()
        .enumerate()
        .filter(|(_, (kind, _))| *kind != ' ')
    {
        let (start, end) = (
            index.saturating_sub(CONTEXT),
            (index + 1 + CONTEXT).min(lines.len()),
        );
        match groups.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => groups.push((start, end)),
        }
    }
    let count = |range: &[(char, &str)], other: char| {
        range.iter().filter(|(kind, _)| *kind != other).count()
    };
    let mut out = Vec::new();
    for (start, end) in groups {
        let (before, range) = (&lines[..start], &lines[start..end]);
        let (old_before, new_before) = (count(before, '+'), count(before, '-'));
        let (old_count, new_count) = (count(range, '+'), count(range, '-'));
        let first = |before: usize, count: usize| if count == 0 { before } else { before + 1 };
        out.push(format!(
            "@@ -{},{old_count} +{},{new_count} @@",
            first(old_before, old_count),
            first(new_before, new_count)
        ));
        out.extend(range.iter().map(|(kind, text)| format!("{kind}{text}")));
    }
    out.join("\n")
}

struct Pull {
    number: i64,
    title: String,
    body: String,
    author: String,
    closed: bool,
    draft: bool,
    merged: bool,
    head_ref: String,
    base_sha: String,
    head_sha: String,
    merge_base: String,
    files: Vec<DemoFile>,
    updated: i64,
    /// (name, status, conclusion)
    checks: Vec<(&'static str, &'static str, Option<&'static str>)>,
    merge_state: &'static str,
    requested: Vec<String>,
}

struct Comment {
    id: i64,
    author: String,
    body: String,
    created: i64,
    /// The review the comment belongs to; a pending review hides it from everyone but its author.
    review: Option<i64>,
}

struct Thread {
    id: i64,
    number: i64,
    path: String,
    side: String,
    line: i64,
    start_line: Option<i64>,
    start_side: Option<String>,
    resolved: bool,
    comments: Vec<Comment>,
}

struct Review {
    id: i64,
    number: i64,
    author: String,
    /// `PENDING`, `APPROVED`, `CHANGES_REQUESTED` or `COMMENTED`.
    state: String,
    body: String,
    submitted: i64,
}

struct Demo {
    pulls: Vec<Pull>,
    threads: Vec<Thread>,
    reviews: Vec<Review>,
    /// Conversation comments: (pull, author, body, time).
    comments: Vec<(i64, String, String, i64)>,
    /// (login, pull, path)
    viewed: HashSet<(String, i64, String)>,
    /// Access token → (GitHub id, login).
    tokens: HashMap<String, (i64, String)>,
    next_id: i64,
}

fn sha(seed: &str) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(seed.as_bytes())
        .iter()
        .take(20)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now_secs() -> i64 {
    TimestampMillis::now().as_millis() / 1000
}

fn iso(secs: i64) -> String {
    jiff::Timestamp::from_second(secs)
        .map(|time| time.to_string())
        .unwrap_or_default()
}

fn avatar(login: &str) -> String {
    let colors = [
        "#e5484d", "#3e63dd", "#30a46c", "#f76b15", "#8e4ec6", "#12a594",
    ];
    let color = colors[login.bytes().map(usize::from).sum::<usize>() % colors.len()];
    let letter = login.chars().next().unwrap_or('?').to_ascii_uppercase();
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='64' height='64'><rect width='64' height='64' fill='{color}'/>\
         <text x='32' y='42' font-family='sans-serif' font-size='30' fill='white' text-anchor='middle'>{letter}</text></svg>"
    );
    format!("data:image/svg+xml;base64,{}", STANDARD.encode(svg))
}

fn author(login: &str) -> Value {
    json!({ "login": login, "avatarUrl": avatar(login) })
}

const LIMIT_RS: &str = "+//! A token bucket for each GitHub App installation, so one noisy repository cannot flood the webhook queue.
+
+use std::collections::HashMap;
+use std::time::{Duration, Instant};
+
+/// Deliveries an installation may send in a burst.
+const BURST: u32 = 60;
+/// How fast the bucket fills again.
+const REFILL_EVERY: Duration = Duration::from_secs(1);
+
+pub struct DeliveryLimiter {
+    buckets: HashMap<i64, Bucket>,
+}
+
+struct Bucket {
+    tokens: u32,
+    filled_at: Instant,
+}
+
+impl DeliveryLimiter {
+    pub fn new() -> Self {
+        Self { buckets: HashMap::new() }
+    }
+
+    /// `true` when the delivery may go on; `false` when GitHub should retry it later.
+    pub fn allow(&mut self, installation: i64, now: Instant) -> bool {
+        let bucket = self.buckets.entry(installation).or_insert(Bucket { tokens: BURST, filled_at: now });
+        let refills = now.duration_since(bucket.filled_at).as_secs() as u32;
+        if refills > 0 {
+            bucket.tokens = (bucket.tokens + refills).min(BURST);
+            bucket.filled_at = now;
+        }
+        if bucket.tokens == 0 {
+            return false;
+        }
+        bucket.tokens -= 1;
+        true
+    }
+}
";

const MOD_RS: &str = " //! GitHub webhooks: verify, then hand each delivery to the queue.

+mod limit;
+
 use axum::http::StatusCode;
+use std::sync::Mutex;
+use std::time::Instant;

 use crate::queue::DeliveryQueue;
+use limit::DeliveryLimiter;

 pub struct WebhookState {
     pub queue: DeliveryQueue,
     pub secret: Vec<u8>,
+    pub limiter: Mutex<DeliveryLimiter>,
 }

 pub async fn receive(state: &WebhookState, installation: i64, body: &[u8], signature: &str) -> StatusCode {
     if !verify(&state.secret, body, signature) {
         return StatusCode::UNAUTHORIZED;
     }
-    state.queue.push(installation, body.to_vec()).await;
-    StatusCode::ACCEPTED
+    let allowed = state.limiter.lock().expect(\"limiter lock\").allow(installation, Instant::now());
+    if !allowed {
+        // GitHub retries a delivery that gets 429, with backoff.
+        return StatusCode::TOO_MANY_REQUESTS;
+    }
+    state.queue.push(installation, body.to_vec()).await;
+    StatusCode::ACCEPTED
 }

 fn verify(secret: &[u8], body: &[u8], signature: &str) -> bool {
     let expected = hmac_sha256(secret, body);
     constant_time_eq(expected.as_bytes(), signature.trim_start_matches(\"sha256=\").as_bytes())
 }
";

const TEST_RS: &str = "+use std::time::{Duration, Instant};
+
+use orbit_server::github::limit::DeliveryLimiter;
+
+#[test]
+fn a_burst_is_allowed_then_refused() {
+    let mut limiter = DeliveryLimiter::new();
+    let now = Instant::now();
+    assert!((0..60).all(|_| limiter.allow(7, now)));
+    assert!(!limiter.allow(7, now));
+}
+
+#[test]
+fn the_bucket_fills_again() {
+    let mut limiter = DeliveryLimiter::new();
+    let now = Instant::now();
+    (0..60).for_each(|_| { limiter.allow(7, now); });
+    assert!(limiter.allow(7, now + Duration::from_secs(2)));
+}
+
+#[test]
+fn installations_have_their_own_buckets() {
+    let mut limiter = DeliveryLimiter::new();
+    let now = Instant::now();
+    (0..60).for_each(|_| { limiter.allow(7, now); });
+    assert!(limiter.allow(8, now));
+}
";

const WEBHOOKS_TSX: &str = " import { Badge } from '@/components/ui/badge'
 import { SettingsCard } from '@/components/settings/SettingsCard'

 export function WebhooksPage({ deliveries }: { deliveries: Delivery[] }) {
   return (
     <SettingsCard title=\"Recent deliveries\">
       {deliveries.map((delivery) => (
         <div key={delivery.id} className=\"flex items-center gap-2\">
           <span className=\"font-mono text-xs\">{delivery.event}</span>
-          <Badge>{delivery.status}</Badge>
+          <Badge variant={delivery.status === 429 ? 'warning' : 'secondary'}>
+            {delivery.status === 429 ? 'Retried later' : delivery.status}
+          </Badge>
         </div>
       ))}
     </SettingsCard>
   )
 }
";

const DOCS_MD: &str = " # GitHub webhooks

 Orbit receives pull request, review and check events from your GitHub App.

-Every delivery is queued at once.
+Every installation may send 60 deliveries in a burst, then one each second.
+A delivery over the limit gets `429 Too Many Requests`, and GitHub sends it again later.

 ## Troubleshooting

 Open **Settings → Webhooks** to see the recent deliveries.
";

const SHORTCUTS_TSX: &str = " export const pullShortcuts = [
   { key: 'j', label: 'Next file' },
   { key: 'k', label: 'Previous file' },
+  { key: 'n', label: 'Next comment' },
+  { key: 'p', label: 'Previous comment' },
+  { key: 'v', label: 'Mark file as viewed' },
 ]
";

const QUEUE_RS: &str = " pub struct DeliveryQueue {
-    capacity: usize,
+    /// Deliveries past this wait in the database instead of in memory.
+    capacity: usize,
+    spilled: u64,
 }
";

const RETRY_RS: &str = " pub fn retry_after(attempt: u32) -> Duration {
-    Duration::from_secs(5)
+    // 5 s, 10 s, 20 s … at most 5 minutes
+    Duration::from_secs(5 * 2u64.pow(attempt.min(6))).min(Duration::from_secs(300))
 }
";

impl Demo {
    fn sample() -> Self {
        let now = now_secs();
        let mut pulls = vec![
            Pull {
                number: 482,
                title: PULL_482_TITLE.to_owned(),
                body: "A busy repository can send hundreds of deliveries in a minute (a rebase of a long branch, a CI \
                       matrix). Today they all go into the queue at once and other workspaces wait.\n\n\
                       ## What changes\n\n- A token bucket for each installation: a burst of **60**, then 1 each second.\n\
                       - Over the limit, Orbit answers `429`; GitHub retries the delivery with backoff.\n\
                       - Settings → Webhooks shows these deliveries as *Retried later*.\n\n\
                       ## How to test\n\n1. `cargo test -p orbit-server --test github_limit`\n2. Send 100 test deliveries \
                       from the GitHub App settings and look at the deliveries list."
                    .to_owned(),
                author: MEMBER.1.to_owned(),
                closed: false,
                draft: false,
                merged: false,
                head_ref: "mia/webhook-rate-limit".to_owned(),
                base_sha: sha("482-base"),
                head_sha: sha("482-head"),
                merge_base: sha("482-merge-base"),
                files: vec![
                    DemoFile::changed("apps/server/src/github/limit.rs", LIMIT_RS),
                    DemoFile::changed("apps/server/src/github/mod.rs", MOD_RS),
                    DemoFile::changed("apps/server/tests/github_limit.rs", TEST_RS),
                    DemoFile::changed("apps/web/src/features/settings/WebhooksPage.tsx", WEBHOOKS_TSX),
                    DemoFile::changed("docs/github-webhooks.md", DOCS_MD).renamed("docs/webhooks.md"),
                    DemoFile::large("bun.lock", 412, 388),
                ],
                updated: now - 25 * 60,
                checks: vec![
                    ("server / test", "COMPLETED", Some("SUCCESS")),
                    ("server / clippy", "COMPLETED", Some("SUCCESS")),
                    ("web / typecheck", "COMPLETED", Some("SUCCESS")),
                    ("e2e", "COMPLETED", Some("SUCCESS")),
                ],
                merge_state: "CLEAN",
                requested: vec![DEVELOPER.1.to_owned()],
            },
            Pull {
                number: 479,
                title: "Keyboard shortcuts for review comments".to_owned(),
                body: "Still a draft: `n` and `p` jump between comments, `v` marks a file as viewed.".to_owned(),
                author: "sam-lee".to_owned(),
                closed: false,
                draft: true,
                merged: false,
                head_ref: "sam/review-shortcuts".to_owned(),
                base_sha: sha("479-base"),
                head_sha: sha("479-head"),
                merge_base: sha("479-merge-base"),
                files: vec![DemoFile::changed("apps/web/src/features/pulls/shortcuts.ts", SHORTCUTS_TSX)],
                updated: now - 3 * 3600,
                checks: vec![("web / typecheck", "IN_PROGRESS", None), ("e2e", "QUEUED", None)],
                merge_state: "DRAFT",
                requested: Vec::new(),
            },
            Pull {
                number: 477,
                title: "Spill the delivery queue to the database when it is full".to_owned(),
                body: "Keeps memory flat when GitHub sends a lot at once.".to_owned(),
                author: DEVELOPER.1.to_owned(),
                closed: false,
                draft: false,
                merged: false,
                head_ref: "dev/queue-spill".to_owned(),
                base_sha: sha("477-base"),
                head_sha: sha("477-head"),
                merge_base: sha("477-merge-base"),
                files: vec![DemoFile::changed("apps/server/src/queue.rs", QUEUE_RS)],
                updated: now - 90 * 60,
                checks: vec![("server / test", "COMPLETED", Some("FAILURE")), ("server / clippy", "COMPLETED", Some("SUCCESS"))],
                merge_state: "BEHIND",
                requested: vec![MEMBER.1.to_owned()],
            },
            Pull {
                number: 470,
                title: "Exponential backoff for webhook retries".to_owned(),
                body: "Retries wait 5 s, 10 s, 20 s and so on, at most 5 minutes.".to_owned(),
                author: DEVELOPER.1.to_owned(),
                closed: true,
                draft: false,
                merged: true,
                head_ref: "dev/retry-backoff".to_owned(),
                base_sha: sha("470-base"),
                head_sha: sha("470-head"),
                merge_base: sha("470-merge-base"),
                files: vec![DemoFile::changed("apps/server/src/retry.rs", RETRY_RS)],
                updated: now - 26 * 3600,
                checks: vec![("server / test", "COMPLETED", Some("SUCCESS"))],
                merge_state: "CLEAN",
                requested: Vec::new(),
            },
            Pull {
                number: 466,
                title: "Try a Redis queue for deliveries".to_owned(),
                body: "Closed: the database queue is enough for now.".to_owned(),
                author: "sam-lee".to_owned(),
                closed: true,
                draft: false,
                merged: false,
                head_ref: "sam/redis-queue".to_owned(),
                base_sha: sha("466-base"),
                head_sha: sha("466-head"),
                merge_base: sha("466-merge-base"),
                files: vec![DemoFile::changed("apps/server/src/retry.rs", RETRY_RS)],
                updated: now - 4 * 86_400,
                checks: Vec::new(),
                merge_state: "CLEAN",
                requested: Vec::new(),
            },
        ];
        pulls.sort_by_key(|pull| std::cmp::Reverse(pull.updated));
        let main = pulls
            .iter()
            .find(|pull| pull.number == 482)
            .expect("sample pull");
        let (limit, module) = (&main.files[0], &main.files[1]);
        let threads = vec![
            Thread {
                id: 1,
                number: 482,
                path: limit.path.clone(),
                side: "RIGHT".to_owned(),
                line: limit.line_of("as_secs() as u32"),
                start_line: None,
                start_side: None,
                resolved: false,
                comments: vec![
                    Comment { id: 9001, author: "sam-lee".to_owned(), body: "This drops the part of a second that is left: after 1.9 s only one token comes back. Is that on purpose?".to_owned(), created: now - 50 * 60, review: Some(1) },
                    Comment { id: 9002, author: MEMBER.1.to_owned(), body: "Yes: a little slower than the limit is fine here, and it keeps the math in whole seconds.".to_owned(), created: now - 40 * 60, review: Some(2) },
                ],
            },
            Thread {
                id: 2,
                number: 482,
                path: module.path.clone(),
                side: "RIGHT".to_owned(),
                line: module.line_of("expect(\"limiter lock\")"),
                start_line: None,
                start_side: None,
                resolved: true,
                comments: vec![Comment {
                    id: 9003,
                    author: "sam-lee".to_owned(),
                    body: "A `std::sync::Mutex` in an async handler is OK while the lock is not held across an `.await` — and it is not. 👍".to_owned(),
                    created: now - 48 * 60,
                    review: Some(1),
                }],
            },
        ];
        let reviews = vec![
            Review {
                id: 1,
                number: 482,
                author: "sam-lee".to_owned(),
                state: "COMMENTED".to_owned(),
                body: "Looks good overall, one question on the refill.".to_owned(),
                submitted: now - 48 * 60,
            },
            Review {
                id: 2,
                number: 482,
                author: MEMBER.1.to_owned(),
                state: "COMMENTED".to_owned(),
                body: String::new(),
                submitted: now - 40 * 60,
            },
            Review {
                id: 3,
                number: 477,
                author: MEMBER.1.to_owned(),
                state: "APPROVED".to_owned(),
                body: "Nice and small. The failing test is the flaky one from #461.".to_owned(),
                submitted: now - 90 * 60,
            },
            Review {
                id: 4,
                number: 470,
                author: MEMBER.1.to_owned(),
                state: "APPROVED".to_owned(),
                body: String::new(),
                submitted: now - 27 * 3600,
            },
        ];
        let comments = vec![
            (482, MEMBER.1.to_owned(), "This is the first part of the webhook work. The queue spill comes in #477.".to_owned(), now - 55 * 60),
            (482, "sam-lee".to_owned(), "Tried it with 300 deliveries from the App settings: 60 went through, the rest came back over the next 4 minutes.".to_owned(), now - 35 * 60),
        ];
        let tokens = [DEVELOPER, MEMBER]
            .into_iter()
            .map(|(id, login, token)| (token.to_owned(), (id, login.to_owned())))
            .collect();
        Self {
            pulls,
            threads,
            reviews,
            comments,
            viewed: HashSet::new(),
            tokens,
            next_id: 10_000,
        }
    }

    fn id(&mut self) -> i64 {
        self.next_id += 1;
        self.next_id
    }

    fn pull(&self, number: i64) -> Option<&Pull> {
        self.pulls.iter().find(|pull| pull.number == number)
    }

    fn pending(&self, review: Option<i64>) -> bool {
        review.is_some_and(|id| {
            self.reviews
                .iter()
                .any(|review| review.id == id && review.state == "PENDING")
        })
    }

    fn comment_json(&self, comment: &Comment, viewer: &str) -> Value {
        json!({
            "id": format!("PRRC_demo_{}", comment.id), "databaseId": comment.id, "body": comment.body,
            "createdAt": iso(comment.created), "state": if self.pending(comment.review) { "PENDING" } else { "SUBMITTED" },
            "viewerCanUpdate": comment.author == viewer, "author": author(&comment.author)
        })
    }

    /// A thread as `viewer` sees it: others' pending comments are hidden. `None` when nothing is left.
    fn thread_json(&self, thread: &Thread, viewer: &str) -> Option<Value> {
        let comments: Vec<Value> = thread
            .comments
            .iter()
            .filter(|comment| comment.author == viewer || !self.pending(comment.review))
            .map(|comment| self.comment_json(comment, viewer))
            .collect();
        (!comments.is_empty()).then(|| {
            json!({
                "id": format!("PRRT_demo_{}", thread.id), "path": thread.path, "line": thread.line,
                "startLine": thread.start_line, "diffSide": thread.side, "startDiffSide": thread.start_side,
                "isResolved": thread.resolved, "isOutdated": false, "viewerCanResolve": true, "viewerCanReply": true,
                "comments": { "nodes": comments }
            })
        })
    }

    fn rollup(pull: &Pull) -> Value {
        if pull.checks.is_empty() {
            return Value::Null;
        }
        let nodes: Vec<Value> = pull
            .checks
            .iter()
            .map(|(name, status, conclusion)| {
                json!({ "__typename": "CheckRun", "name": name, "status": status, "conclusion": conclusion,
                        "detailsUrl": format!("https://github.com/{DEMO_REPOSITORY}/actions") })
            })
            .collect();
        let state = if pull.checks.iter().any(|check| check.2 == Some("FAILURE")) {
            "FAILURE"
        } else if pull.checks.iter().any(|check| check.1 != "COMPLETED") {
            "PENDING"
        } else {
            "SUCCESS"
        };
        json!({ "state": state, "contexts": { "nodes": nodes } })
    }

    fn detail(&self, pull: &Pull) -> Value {
        json!({
            "id": format!("PR_demo_{}", pull.number), "number": pull.number, "title": pull.title, "body": pull.body,
            "state": if pull.merged { "MERGED" } else if pull.closed { "CLOSED" } else { "OPEN" },
            "isDraft": pull.draft, "merged": pull.merged, "locked": false,
            "url": format!("https://github.com/{DEMO_REPOSITORY}/pull/{}", pull.number),
            "additions": pull.files.iter().map(|file| file.additions).sum::<i64>(),
            "deletions": pull.files.iter().map(|file| file.deletions).sum::<i64>(),
            "changedFiles": pull.files.len(), "baseRefName": "main", "headRefName": pull.head_ref,
            "baseRefOid": pull.base_sha, "headRefOid": pull.head_sha,
            "mergeable": if pull.merge_state == "DIRTY" { "CONFLICTING" } else { "MERGEABLE" },
            "mergeStateStatus": pull.merge_state, "author": author(&pull.author),
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": Self::rollup(pull) } }] }
        })
    }
}

// ---- the HTTP side ----

type Shared = Arc<Mutex<Demo>>;

fn reply(status: u16, body: Value) -> Response {
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        axum::Json(body),
    )
        .into_response()
}

fn not_found() -> Response {
    reply(404, json!({ "message": "Not Found" }))
}

fn graphql_error(kind: &str, message: &str) -> Response {
    reply(
        200,
        json!({ "errors": [{ "type": kind, "message": message }] }),
    )
}

fn data(value: Value) -> Response {
    reply(200, json!({ "data": value }))
}

async fn handle(State(demo): State<Shared>, request: Request<Body>) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let auth = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_owned();
    let bytes = to_bytes(request.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap_or_default();
    let query: HashMap<String, String> = uri
        .query()
        .map(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default();
    let mut demo = demo
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = uri.path();
    // OAuth, for a member who connects again: any code gives a new demo account.
    if path == "/github/web/login/oauth/authorize" {
        let target = format!(
            "{}?code=demo&state={}",
            query.get("redirect_uri").map_or("", String::as_str),
            query.get("state").map_or("", String::as_str)
        );
        return (StatusCode::FOUND, [(header::LOCATION, target)]).into_response();
    }
    if path == "/github/web/login/oauth/access_token" {
        let id = demo.id();
        let token = format!("demo-user-{id}");
        demo.tokens
            .insert(token.clone(), (id, format!("demo-user-{id}")));
        return reply(
            200,
            json!({ "access_token": token, "token_type": "bearer", "scope": "" }),
        );
    }
    let Some(path) = path.strip_prefix("/github/api/") else {
        return not_found();
    };
    let Some((_, viewer)) = demo.tokens.get(&auth).cloned() else {
        return reply(401, json!({ "message": "Bad credentials" }));
    };
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if path == "graphql" {
        return graphql(&mut demo, &viewer, &body);
    }
    if path == "user" {
        let (id, login) = demo.tokens[&auth].clone();
        return reply(
            200,
            json!({ "id": id, "login": login, "avatar_url": avatar(&login) }),
        );
    }
    if path == "search/issues" {
        return search(&demo, &viewer, query.get("q").map_or("", String::as_str));
    }
    let Some(rest) = path.strip_prefix("repos/") else {
        return not_found();
    };
    let mut parts = rest.splitn(3, '/');
    let repository = format!(
        "{}/{}",
        parts.next().unwrap_or(""),
        parts.next().unwrap_or("")
    )
    .to_lowercase();
    let rest = parts.next().unwrap_or("");
    if repository != DEMO_REPOSITORY {
        return not_found();
    }
    repository_route(&mut demo, &viewer, method.as_str(), rest, &query, &body)
}

fn search(demo: &Demo, viewer: &str, q: &str) -> Response {
    let words: Vec<&str> = q.split_whitespace().collect();
    if !words
        .iter()
        .any(|word| word.eq_ignore_ascii_case(&format!("repo:{DEMO_REPOSITORY}")))
    {
        return reply(200, json!({ "total_count": 0, "items": [] }));
    }
    let text: Vec<String> = words
        .iter()
        .filter(|word| !word.contains(':'))
        .map(|word| word.to_lowercase())
        .collect();
    let items: Vec<Value> = demo
        .pulls
        .iter()
        .filter(|pull| {
            words.iter().all(|word| match *word {
                "is:open" => !pull.closed,
                "is:closed" => pull.closed,
                "is:merged" => pull.merged,
                "is:unmerged" => !pull.merged,
                "author:@me" => pull.author == viewer,
                "review-requested:@me" => pull.requested.iter().any(|login| login == viewer),
                _ => true,
            }) && text.iter().all(|word| pull.title.to_lowercase().contains(word.as_str()))
        })
        .map(|pull| {
            let comments = demo.comments.iter().filter(|comment| comment.0 == pull.number).count();
            json!({
                "number": pull.number, "title": pull.title, "state": if pull.closed { "closed" } else { "open" },
                "draft": pull.draft, "pull_request": { "merged_at": pull.merged.then(|| iso(pull.updated)) },
                "user": { "login": pull.author, "avatar_url": avatar(&pull.author) }, "updated_at": iso(pull.updated),
                "comments": comments, "repository_url": format!("https://api.github.com/repos/{DEMO_REPOSITORY}")
            })
        })
        .collect();
    reply(200, json!({ "total_count": items.len(), "items": items }))
}

fn repository_route(
    demo: &mut Demo,
    viewer: &str,
    method: &str,
    rest: &str,
    query: &HashMap<String, String>,
    body: &Value,
) -> Response {
    let segments: Vec<&str> = rest.split('/').collect();
    match (method, segments.as_slice()) {
        ("GET", ["contents", path @ ..]) => {
            let path = path.join("/").replace("%20", " ");
            let wanted = query.get("ref").map_or("", String::as_str);
            let text = demo.pulls.iter().find_map(|pull| {
                let file = pull.files.iter().find(|file| {
                    file.path == path || file.previous_path.as_deref() == Some(path.as_str())
                })?;
                if wanted == pull.head_sha {
                    file.new.clone()
                } else if wanted == pull.merge_base || wanted == pull.base_sha {
                    file.old.clone()
                } else {
                    None
                }
            });
            match text {
                Some(text) => reply(
                    200,
                    json!({ "type": "file", "encoding": "base64", "size": text.len(), "content": STANDARD.encode(text) }),
                ),
                None => not_found(),
            }
        }
        ("GET", ["compare", range]) => {
            let (base, head) = range.split_once("...").unwrap_or_default();
            match demo
                .pulls
                .iter()
                .find(|pull| pull.base_sha == base && pull.head_sha == head)
            {
                Some(pull) => reply(
                    200,
                    json!({ "merge_base_commit": { "sha": pull.merge_base } }),
                ),
                None => not_found(),
            }
        }
        ("DELETE", ["git", "refs", "heads", ..]) => StatusCode::NO_CONTENT.into_response(),
        (_, ["pulls", number, tail @ ..]) => {
            let Ok(number) = number.parse::<i64>() else {
                return not_found();
            };
            if demo.pull(number).is_none() {
                return not_found();
            }
            pull_route(demo, viewer, method, number, tail, query, body)
        }
        _ => not_found(),
    }
}

fn pull_route(
    demo: &mut Demo,
    viewer: &str,
    method: &str,
    number: i64,
    tail: &[&str],
    query: &HashMap<String, String>,
    body: &Value,
) -> Response {
    let now = now_secs();
    match (method, tail) {
        ("GET", []) => {
            let pull = demo.pull(number).expect("checked");
            let repo = json!({ "full_name": DEMO_REPOSITORY });
            reply(
                200,
                json!({ "merged": pull.merged, "head": { "ref": pull.head_ref, "repo": repo }, "base": { "ref": "main", "repo": repo } }),
            )
        }
        ("GET", ["files"]) => {
            if query.get("page").is_some_and(|page| page != "1") {
                return reply(200, json!([]));
            }
            let files: Vec<Value> = demo
                .pull(number)
                .expect("checked")
                .files
                .iter()
                .map(|file| {
                    json!({ "filename": file.path, "previous_filename": file.previous_path, "status": file.status,
                            "additions": file.additions, "deletions": file.deletions, "patch": file.patch })
                })
                .collect();
            reply(200, Value::Array(files))
        }
        ("POST", ["comments"]) => {
            let id = demo.id();
            let comment = Comment {
                id,
                author: viewer.to_owned(),
                body: body["body"].as_str().unwrap_or_default().to_owned(),
                created: now,
                review: None,
            };
            demo.threads.push(Thread {
                id,
                number,
                path: body["path"].as_str().unwrap_or_default().to_owned(),
                side: body["side"].as_str().unwrap_or("RIGHT").to_owned(),
                line: body["line"].as_i64().unwrap_or(1),
                start_line: body["start_line"].as_i64(),
                start_side: body["start_side"].as_str().map(str::to_owned),
                resolved: false,
                comments: vec![comment],
            });
            reply(201, json!({ "id": id }))
        }
        ("PUT", ["merge"]) => {
            let pull = demo
                .pulls
                .iter_mut()
                .find(|pull| pull.number == number)
                .expect("checked");
            if pull.closed || pull.draft || pull.merge_state == "DIRTY" {
                return reply(405, json!({ "message": "Pull Request is not mergeable" }));
            }
            if body["sha"].as_str() != Some(pull.head_sha.as_str()) {
                return reply(
                    409,
                    json!({ "message": "Head branch was modified. Review and try the merge again." }),
                );
            }
            pull.merged = true;
            pull.closed = true;
            pull.updated = now;
            reply(
                200,
                json!({ "sha": sha(&format!("merge-{number}")), "merged": true, "message": "Pull Request successfully merged" }),
            )
        }
        _ => not_found(),
    }
}

fn node(value: &Value, prefix: &str) -> Option<i64> {
    value.as_str()?.strip_prefix(prefix)?.parse().ok()
}

fn review_state(event: &str) -> &'static str {
    match event {
        "APPROVE" => "APPROVED",
        "REQUEST_CHANGES" => "CHANGES_REQUESTED",
        _ => "COMMENTED",
    }
}

fn graphql(demo: &mut Demo, viewer: &str, body: &Value) -> Response {
    let text = body["query"].as_str().unwrap_or_default();
    let operation = text
        .split_whitespace()
        .nth(1)
        .and_then(|word| word.split('(').next())
        .unwrap_or_default();
    let vars = &body["variables"];
    let number = vars["number"]
        .as_i64()
        .or_else(|| node(&vars["pullId"], "PR_demo_"));
    let in_repo = vars["owner"].is_null()
        || format!(
            "{}/{}",
            vars["owner"].as_str().unwrap_or(""),
            vars["repo"].as_str().unwrap_or("")
        )
        .to_lowercase()
            == DEMO_REPOSITORY;
    let pull = number
        .filter(|_| in_repo)
        .and_then(|number| demo.pull(number));
    let now = now_secs();
    match operation {
        "PullDetail" => {
            let repository = in_repo.then(|| {
                json!({
                    "mergeCommitAllowed": true, "squashMergeAllowed": true, "rebaseMergeAllowed": true,
                    "viewerDefaultMergeMethod": "SQUASH", "viewerPermission": "WRITE",
                    "pullRequest": pull.map(|pull| demo.detail(pull))
                })
            });
            data(json!({ "viewer": { "login": viewer }, "repository": repository }))
        }
        "PullBasics" => data(json!({
            "viewer": { "login": viewer },
            "repository": { "pullRequest": pull.map(|pull| json!({ "id": format!("PR_demo_{}", pull.number), "headRefOid": pull.head_sha, "author": { "login": pull.author } })) }
        })),
        "PullId" => data(
            json!({ "repository": { "pullRequest": pull.map(|pull| json!({ "id": format!("PR_demo_{}", pull.number) })) } }),
        ),
        "PullFiles" => {
            let Some(pull) = pull else {
                return graphql_error("NOT_FOUND", "no such pull request");
            };
            let nodes: Vec<Value> = pull
                .files
                .iter()
                .map(|file| {
                    let viewed = demo.viewed.contains(&(viewer.to_owned(), pull.number, file.path.clone()));
                    json!({ "path": file.path, "viewerViewedState": if viewed { "VIEWED" } else { "UNVIEWED" } })
                })
                .collect();
            data(
                json!({ "repository": { "pullRequest": { "files": { "nodes": nodes, "pageInfo": { "hasNextPage": false, "endCursor": null } } } } }),
            )
        }
        "PullThreads" => {
            let Some(pull) = pull else {
                return graphql_error("NOT_FOUND", "no such pull request");
            };
            let nodes: Vec<Value> = demo
                .threads
                .iter()
                .filter(|thread| thread.number == pull.number)
                .filter_map(|thread| demo.thread_json(thread, viewer))
                .collect();
            data(
                json!({ "repository": { "pullRequest": { "reviewThreads": { "nodes": nodes, "pageInfo": { "hasNextPage": false, "endCursor": null } } } } }),
            )
        }
        "PullConversation" => {
            let Some(pull) = pull else {
                return graphql_error("NOT_FOUND", "no such pull request");
            };
            let comments: Vec<Value> = demo
                .comments
                .iter()
                .filter(|comment| comment.0 == pull.number)
                .map(|comment| json!({ "body": comment.2, "createdAt": iso(comment.3), "author": author(&comment.1) }))
                .collect();
            let reviews: Vec<Value> = demo
                .reviews
                .iter()
                .filter(|review| review.number == pull.number && review.state != "PENDING")
                .map(|review| json!({ "body": review.body, "state": review.state, "submittedAt": iso(review.submitted), "author": author(&review.author) }))
                .collect();
            data(
                json!({ "repository": { "pullRequest": { "comments": { "nodes": comments }, "reviews": { "nodes": reviews } } } }),
            )
        }
        "PendingReview" => {
            let Some(pull) = pull else {
                return graphql_error("NOT_FOUND", "no such pull request");
            };
            let nodes: Vec<Value> = demo
                .reviews
                .iter()
                .filter(|review| review.number == pull.number && review.state == "PENDING" && review.author == viewer)
                .map(|review| {
                    let count = demo.threads.iter().flat_map(|thread| &thread.comments).filter(|comment| comment.review == Some(review.id)).count();
                    json!({ "id": format!("PRR_demo_{}", review.id), "viewerDidAuthor": true, "comments": { "totalCount": count } })
                })
                .collect();
            data(json!({ "repository": { "pullRequest": { "reviews": { "nodes": nodes } } } }))
        }
        "MarkViewed" | "UnmarkViewed" => {
            let (Some(number), Some(path)) = (number, vars["path"].as_str()) else {
                return graphql_error("NOT_FOUND", "no such file");
            };
            let key = (viewer.to_owned(), number, path.to_owned());
            if operation == "MarkViewed" {
                demo.viewed.insert(key);
            } else {
                demo.viewed.remove(&key);
            }
            data(json!({ "markFileAsViewed": { "clientMutationId": null } }))
        }
        "AddReview" => {
            let Some(number) = pull.map(|pull| pull.number) else {
                return graphql_error("NOT_FOUND", "no such pull request");
            };
            let id = demo.id();
            let state = vars["event"].as_str().map_or("PENDING", review_state);
            demo.reviews.push(Review {
                id,
                number,
                author: viewer.to_owned(),
                state: state.to_owned(),
                body: vars["body"].as_str().unwrap_or_default().to_owned(),
                submitted: now,
            });
            data(
                json!({ "addPullRequestReview": { "pullRequestReview": { "id": format!("PRR_demo_{id}") } } }),
            )
        }
        "AddThread" => {
            let Some(review) = node(&vars["reviewId"], "PRR_demo_") else {
                return graphql_error("NOT_FOUND", "no such review");
            };
            let Some(number) = demo
                .reviews
                .iter()
                .find(|item| item.id == review)
                .map(|item| item.number)
            else {
                return graphql_error("NOT_FOUND", "no such review");
            };
            let id = demo.id();
            let thread = Thread {
                id,
                number,
                path: vars["path"].as_str().unwrap_or_default().to_owned(),
                side: vars["side"].as_str().unwrap_or("RIGHT").to_owned(),
                line: vars["line"].as_i64().unwrap_or(1),
                start_line: vars["startLine"].as_i64(),
                start_side: vars["startSide"].as_str().map(str::to_owned),
                resolved: false,
                comments: vec![Comment {
                    id,
                    author: viewer.to_owned(),
                    body: vars["body"].as_str().unwrap_or_default().to_owned(),
                    created: now,
                    review: Some(review),
                }],
            };
            let json = demo.thread_json(&thread, viewer);
            demo.threads.push(thread);
            data(json!({ "addPullRequestReviewThread": { "thread": json } }))
        }
        "AddReply" => {
            let Some(thread) = node(&vars["threadId"], "PRRT_demo_") else {
                return graphql_error("NOT_FOUND", "no such thread");
            };
            let id = demo.id();
            let comment = Comment {
                id,
                author: viewer.to_owned(),
                body: vars["body"].as_str().unwrap_or_default().to_owned(),
                created: now,
                review: node(&vars["reviewId"], "PRR_demo_"),
            };
            let json = demo.comment_json(&comment, viewer);
            let Some(thread) = demo.threads.iter_mut().find(|item| item.id == thread) else {
                return graphql_error("NOT_FOUND", "no such thread");
            };
            thread.comments.push(comment);
            data(json!({ "addPullRequestReviewThreadReply": { "comment": json } }))
        }
        "UpdateComment" | "DeleteComment" => {
            let Some(id) = node(&vars["id"], "PRRC_demo_") else {
                return graphql_error("NOT_FOUND", "no such comment");
            };
            let Some(thread) = demo
                .threads
                .iter()
                .position(|thread| thread.comments.iter().any(|comment| comment.id == id))
            else {
                return graphql_error("NOT_FOUND", "no such comment");
            };
            let index = demo.threads[thread]
                .comments
                .iter()
                .position(|comment| comment.id == id)
                .expect("found");
            if demo.threads[thread].comments[index].author != viewer {
                return graphql_error("FORBIDDEN", "You can only change your own comments.");
            }
            if operation == "DeleteComment" {
                demo.threads[thread].comments.remove(index);
                if demo.threads[thread].comments.is_empty() {
                    demo.threads.remove(thread);
                }
                return data(
                    json!({ "deletePullRequestReviewComment": { "pullRequestReview": null } }),
                );
            }
            demo.threads[thread].comments[index].body =
                vars["body"].as_str().unwrap_or_default().to_owned();
            let json = demo.comment_json(&demo.threads[thread].comments[index], viewer);
            data(json!({ "updatePullRequestReviewComment": { "pullRequestReviewComment": json } }))
        }
        "ResolveThread" | "UnresolveThread" => {
            let Some(id) = node(&vars["id"], "PRRT_demo_") else {
                return graphql_error("NOT_FOUND", "no such thread");
            };
            let Some(thread) = demo.threads.iter_mut().find(|thread| thread.id == id) else {
                return graphql_error("NOT_FOUND", "no such thread");
            };
            thread.resolved = operation == "ResolveThread";
            let thread = demo
                .threads
                .iter()
                .find(|thread| thread.id == id)
                .expect("found");
            let field = if operation == "ResolveThread" {
                "resolveReviewThread"
            } else {
                "unresolveReviewThread"
            };
            data(json!({ field: { "thread": demo.thread_json(thread, viewer) } }))
        }
        "SubmitReview" | "DeleteReview" => {
            let key = if operation == "SubmitReview" {
                &vars["reviewId"]
            } else {
                &vars["id"]
            };
            let Some(id) = node(key, "PRR_demo_") else {
                return graphql_error("NOT_FOUND", "no such review");
            };
            let Some(index) = demo
                .reviews
                .iter()
                .position(|review| review.id == id && review.author == viewer)
            else {
                return graphql_error("NOT_FOUND", "no such review");
            };
            if operation == "DeleteReview" {
                demo.reviews.remove(index);
                for thread in &mut demo.threads {
                    thread.comments.retain(|comment| comment.review != Some(id));
                }
                demo.threads.retain(|thread| !thread.comments.is_empty());
                return data(
                    json!({ "deletePullRequestReview": { "pullRequestReview": { "id": format!("PRR_demo_{id}") } } }),
                );
            }
            let review = &mut demo.reviews[index];
            review.state = review_state(vars["event"].as_str().unwrap_or("COMMENT")).to_owned();
            review.body = vars["body"].as_str().unwrap_or_default().to_owned();
            review.submitted = now;
            data(
                json!({ "submitPullRequestReview": { "pullRequestReview": { "id": format!("PRR_demo_{id}") } } }),
            )
        }
        _ => graphql_error("NOT_FOUND", "the demo GitHub does not know this query"),
    }
}

#[cfg(test)]
mod tests {
    use super::patch;

    #[test]
    fn a_patch_has_hunks_with_three_lines_of_context() {
        let marked = " a\n b\n c\n d\n-e\n+E\n f\n g\n h\n i\n j\n k\n l\n m\n+n";
        let lines: Vec<(char, &str)> = marked
            .lines()
            .map(|line| (line.chars().next().unwrap(), &line[1..]))
            .collect();
        assert_eq!(
            patch(&lines),
            "@@ -2,7 +2,7 @@\n b\n c\n d\n-e\n+E\n f\n g\n h\n@@ -11,3 +11,4 @@\n k\n l\n m\n+n"
        );
    }

    #[test]
    fn an_added_file_starts_at_zero() {
        let lines = [('+', "x"), ('+', "y")];
        assert_eq!(patch(&lines), "@@ -0,0 +1,2 @@\n+x\n+y");
    }
}
