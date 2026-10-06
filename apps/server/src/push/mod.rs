//! Notifications that reach a user who is not looking at the event: a signal to the tabs where
//! they are active now, or else a Web Push message to every browser they subscribed with.
//!
//! - [`webpush`]: the encryption and the VAPID header.
//! - [`PushService`]: who is notified (their preferences, "do not disturb"), and where.
//!
//! Chat messages are notified when they are sent ([`PushService::chat_message`]). Inbox
//! notifications (task assigned, mentions) are picked up from the `notifications` table by
//! [`PushService::run_inbox_service`], so their writers do not know about push.
//!
//! A push is best effort: a busy push service gets it a second time, and a restart loses the
//! ones that wait. A notification is worth little minutes later, and the unread counts and
//! the Inbox show what was missed, so pushes are not kept in the job queue.

pub mod webpush;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use orbit_platform::{Database, Id, TimestampMillis};
use p256::SecretKey;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use crate::auth_routes::avatar_url;
use crate::live::LiveHub;
use crate::repositories::chat::MessageRecord;

/// The hosts of the push services of the browser vendors. A subscription names the URL the
/// server sends to, so it must not be able to name any other host.
const PUSH_HOSTS: [&str; 5] = [
    "fcm.googleapis.com",
    "updates.push.services.mozilla.com",
    ".push.services.mozilla.com",
    ".notify.windows.com",
    ".push.apple.com",
];
/// How long a push service keeps a message for a browser that is not reachable.
const TTL_SECONDS: u32 = 24 * 60 * 60;
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const INBOX_INTERVAL: Duration = Duration::from_secs(2);
const TITLE_CHARS: usize = 120;
const BODY_CHARS: usize = 180;
/// The browsers one user gets pushes on. The oldest go when another subscribes.
const MAX_SUBSCRIPTIONS: i64 = 20;
/// Pushes to a subscription that may fail in a row before it is removed.
const MAX_FAILURES: i64 = 5;
/// How long a push waits before it goes again to a push service that was busy.
const RETRY_AFTER: Duration = Duration::from_secs(1);
/// The members of a conversation who are notified of a message at the same time.
const CONCURRENT_MEMBERS: usize = 8;
/// The channel tokens of one message that are looked up; the others show as `#channel`.
const CHANNEL_LOOKUPS: usize = 10;

/// An event a user can turn off; the name of its column in `notification_prefs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushKind {
    DirectMessages,
    ChatMentions,
    ThreadReplies,
    ChannelMessages,
    TaskAssigned,
    Mentions,
}

impl PushKind {
    pub const ALL: [Self; 6] = [
        Self::DirectMessages,
        Self::ChatMentions,
        Self::ThreadReplies,
        Self::ChannelMessages,
        Self::TaskAssigned,
        Self::Mentions,
    ];

    #[must_use]
    pub const fn column(self) -> &'static str {
        match self {
            Self::DirectMessages => "direct_messages",
            Self::ChatMentions => "chat_mentions",
            Self::ThreadReplies => "thread_replies",
            Self::ChannelMessages => "channel_messages",
            Self::TaskAssigned => "task_assigned",
            Self::Mentions => "mentions",
        }
    }

    /// The sound an open tab plays: `mention` for what is addressed to the user.
    const fn sound(self) -> &'static str {
        match self {
            Self::ThreadReplies | Self::ChannelMessages => "message",
            _ => "mention",
        }
    }
}

/// What a notification shows. The service worker and an open tab take the same shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// The page a click opens, as a path of the web app.
    pub url: String,
    /// Notifications with the same tag replace each other.
    pub tag: String,
    pub sound: &'static str,
    /// The picture of who caused it, as a path of the app; absent when they have none (the
    /// app's icon shows then).
    pub icon: Option<String>,
}

impl Notice {
    /// The notice with a title and a body short enough for a push message: a task or page
    /// title can be of any length.
    fn clamped(&self) -> Self {
        Self {
            title: short(&self.title, TITLE_CHARS),
            body: short(&self.body, BODY_CHARS),
            ..self.clone()
        }
    }
}

/// `text` in at most `chars` characters; a longer one is cut and ends with `…`.
fn short(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_owned();
    }
    let mut short: String = text.chars().take(chars.saturating_sub(1)).collect();
    short.push('…');
    short
}

#[derive(Clone, Debug)]
pub struct Subscription {
    pub id: Id,
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

/// A browser the user gets pushes on.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct PushSubscriptionRecord {
    #[schema(value_type = String)]
    pub id: Id,
    /// The URL of the browser's `PushSubscription`; a browser finds its own row by it.
    pub endpoint: String,
    pub label: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
}

/// Which events notify the user. They are all on until the user turns one off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NotificationPrefs {
    pub direct_messages: bool,
    pub chat_mentions: bool,
    pub thread_replies: bool,
    /// Every message of a channel whose notify level is "all".
    pub channel_messages: bool,
    pub task_assigned: bool,
    /// Mentions in task comments, pages and page comments.
    pub mentions: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    /// The browser unsubscribed: the subscription can go.
    Gone,
    /// The push service refused the push: the same push would be refused again.
    Failed(String),
    /// The push service is busy or did not answer: the push can go again in a moment.
    Busy(String),
}

/// The server's identity towards the push services.
#[derive(Clone)]
pub struct Vapid {
    pub key: SecretKey,
    pub subject: String,
}

pub trait PushSender: Send + Sync {
    fn send(
        &self,
        vapid: Vapid,
        subscription: Subscription,
        payload: Vec<u8>,
    ) -> BoxFuture<'static, Result<(), SendError>>;
}

/// Whether a subscription's endpoint is at a push service of a browser vendor.
#[must_use]
pub fn allowed_endpoint(endpoint: &str) -> bool {
    url::Url::parse(endpoint).is_ok_and(|url| {
        url.scheme() == "https"
            && url.port().is_none()
            && url.host_str().is_some_and(|host| {
                PUSH_HOSTS
                    .iter()
                    .any(|allowed| match allowed.strip_prefix('.') {
                        Some(_) => host.ends_with(allowed),
                        None => host == *allowed,
                    })
            })
    })
}

/// Sends through the push services over HTTPS.
pub struct WebPushSender {
    client: reqwest::Client,
}

impl WebPushSender {
    /// The client does not follow redirects: only the endpoint itself was checked, and a
    /// redirect could name any other host.
    fn new() -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the HTTP client of the push sender");
        Self { client }
    }
}

impl PushSender for WebPushSender {
    fn send(
        &self,
        vapid: Vapid,
        subscription: Subscription,
        payload: Vec<u8>,
    ) -> BoxFuture<'static, Result<(), SendError>> {
        let client = self.client.clone();
        Box::pin(async move {
            if !allowed_endpoint(&subscription.endpoint) {
                return Err(SendError::Gone);
            }
            let url = url::Url::parse(&subscription.endpoint).map_err(|_| SendError::Gone)?;
            let (Some(ua_public), Some(auth)) = (
                webpush::decode(&subscription.p256dh),
                webpush::decode(&subscription.auth),
            ) else {
                return Err(SendError::Gone);
            };
            let mut salt = [0_u8; 16];
            rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut salt);
            let sender = SecretKey::random(&mut rand_core::OsRng);
            let body = webpush::encrypt(&ua_public, &auth, &payload, &sender, &salt)
                .ok_or(SendError::Gone)?;
            let expires = TimestampMillis::now().as_millis() / 1000 + 12 * 60 * 60;
            let audience = url.origin().ascii_serialization();
            let response = client
                .post(url)
                .header("TTL", TTL_SECONDS)
                .header("Content-Encoding", "aes128gcm")
                .header("Content-Type", "application/octet-stream")
                .header(
                    "Authorization",
                    webpush::vapid_authorization(&vapid.key, &audience, &vapid.subject, expires),
                )
                .body(body)
                .timeout(SEND_TIMEOUT)
                .send()
                .await
                .map_err(|_| SendError::Busy("the push service did not answer".to_owned()))?;
            match response.status().as_u16() {
                200..=299 => Ok(()),
                404 | 410 => Err(SendError::Gone),
                status @ (429 | 500..=599) => {
                    Err(SendError::Busy(format!("push service status {status}")))
                }
                status => Err(SendError::Failed(format!("push service status {status}"))),
            }
        })
    }
}

struct Inner {
    sender: Arc<dyn PushSender>,
    key: OnceCell<SecretKey>,
    subject: OnceLock<String>,
}

#[derive(Clone)]
pub struct PushService {
    database: Database,
    hub: LiveHub,
    inner: Arc<Inner>,
}

impl PushService {
    /// The push service of this database (one for each open database; made on first use).
    #[must_use]
    pub fn of(database: &Database) -> Self {
        // The sender and its HTTP client are made once, with the service.
        Self::with(database, || Arc::new(WebPushSender::new()))
    }

    /// As [`Self::of`], with another sender if the database has no push service yet (tests).
    #[must_use]
    pub fn with_sender(database: &Database, sender: Arc<dyn PushSender>) -> Self {
        Self::with(database, move || sender)
    }

    fn with(database: &Database, sender: impl FnOnce() -> Arc<dyn PushSender>) -> Self {
        let inner = database.extension(move || Inner {
            sender: sender(),
            key: OnceCell::new(),
            subject: OnceLock::new(),
        });
        Self {
            database: database.clone(),
            hub: LiveHub::of(database),
            inner,
        }
    }

    /// The contact the push services get: the public origin of an HTTPS installation.
    pub fn set_origin(&self, origin: &str) {
        if origin.starts_with("https://") {
            let _ = self.inner.subject.set(origin.to_owned());
        }
    }

    async fn vapid(&self) -> Result<Vapid, sqlx::Error> {
        let key = self
            .inner
            .key
            .get_or_try_init(|| async {
                let fresh = SecretKey::random(&mut rand_core::OsRng);
                sqlx::query(
                    "INSERT OR IGNORE INTO push_keys (id, private_key, created_at) VALUES (1, ?, ?)",
                )
                .bind(fresh.to_bytes().as_slice())
                .bind(TimestampMillis::now().as_millis())
                .execute(self.database.pool())
                .await?;
                let stored: Vec<u8> =
                    sqlx::query_scalar("SELECT private_key FROM push_keys WHERE id = 1")
                        .fetch_one(self.database.pool())
                        .await?;
                SecretKey::from_slice(&stored).map_err(|error| sqlx::Error::Decode(Box::new(error)))
            })
            .await?
            .clone();
        let subject = self
            .inner
            .subject
            .get()
            .cloned()
            .unwrap_or_else(|| "mailto:orbit@localhost.invalid".to_owned());
        Ok(Vapid { key, subject })
    }

    /// The key a browser subscribes with (`applicationServerKey`).
    pub async fn public_key(&self) -> Result<String, sqlx::Error> {
        Ok(webpush::public_key(&self.vapid().await?.key))
    }

    pub async fn subscriptions(
        &self,
        user_id: Id,
    ) -> Result<Vec<PushSubscriptionRecord>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, endpoint, label, created_at FROM push_subscriptions WHERE user_id = ? \
             ORDER BY id",
        )
        .bind(user_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(PushSubscriptionRecord {
                    id: row
                        .get::<String, _>("id")
                        .parse()
                        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
                    endpoint: row.get("endpoint"),
                    label: row.get("label"),
                    created_at: TimestampMillis::from_millis(row.get("created_at")),
                })
            })
            .collect()
    }

    /// Stores a browser's subscription for the user and the session. A browser has one
    /// endpoint: its row moves to whoever subscribes with it last. A user keeps the newest
    /// [`MAX_SUBSCRIPTIONS`] only. Callers check the endpoint and the keys first.
    #[allow(clippy::too_many_arguments)]
    pub async fn subscribe(
        &self,
        user_id: Id,
        session_id: Id,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        label: &str,
        now: TimestampMillis,
    ) -> Result<PushSubscriptionRecord, sqlx::Error> {
        let mut tx = self.database.pool().begin().await?;
        let row = sqlx::query(
            "INSERT INTO push_subscriptions (id, user_id, session_id, endpoint, p256dh, auth, label, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (endpoint) DO UPDATE SET \
             user_id = excluded.user_id, session_id = excluded.session_id, p256dh = excluded.p256dh, \
             auth = excluded.auth, label = excluded.label RETURNING id, created_at",
        )
        .bind(Id::new_v7().to_string())
        .bind(user_id.to_string())
        .bind(session_id.to_string())
        .bind(endpoint)
        .bind(p256dh)
        .bind(auth)
        .bind(label)
        .bind(now.as_millis())
        .fetch_one(&mut *tx)
        .await?;
        let id: String = row.get("id");
        // The oldest ones above the limit go; never the one that subscribed now.
        sqlx::query(
            "DELETE FROM push_subscriptions WHERE user_id = ?1 AND id NOT IN ( \
             SELECT id FROM push_subscriptions WHERE user_id = ?1 \
             ORDER BY id = ?2 DESC, created_at DESC, id DESC LIMIT ?3)",
        )
        .bind(user_id.to_string())
        .bind(&id)
        .bind(MAX_SUBSCRIPTIONS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(PushSubscriptionRecord {
            id: id
                .parse()
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
            endpoint: endpoint.to_owned(),
            label: label.to_owned(),
            created_at: TimestampMillis::from_millis(row.get("created_at")),
        })
    }

    /// Removes one of the user's subscriptions; nothing happens when it is not theirs.
    pub async fn unsubscribe(&self, user_id: Id, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM push_subscriptions WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id.to_string())
            .execute(self.database.pool())
            .await?;
        Ok(())
    }

    pub async fn prefs(&self, user_id: Id) -> Result<NotificationPrefs, sqlx::Error> {
        let row = sqlx::query(
            "SELECT direct_messages, chat_mentions, thread_replies, channel_messages, task_assigned, \
             mentions FROM notification_prefs WHERE user_id = ?",
        )
        .bind(user_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        let on = |column: &str| {
            row.as_ref()
                .is_none_or(|row| row.get::<i64, _>(column) == 1)
        };
        Ok(NotificationPrefs {
            direct_messages: on("direct_messages"),
            chat_mentions: on("chat_mentions"),
            thread_replies: on("thread_replies"),
            channel_messages: on("channel_messages"),
            task_assigned: on("task_assigned"),
            mentions: on("mentions"),
        })
    }

    pub async fn set_prefs(
        &self,
        user_id: Id,
        prefs: &NotificationPrefs,
        now: TimestampMillis,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO notification_prefs (user_id, direct_messages, chat_mentions, thread_replies, \
             channel_messages, task_assigned, mentions, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (user_id) DO UPDATE SET direct_messages = excluded.direct_messages, \
             chat_mentions = excluded.chat_mentions, thread_replies = excluded.thread_replies, \
             channel_messages = excluded.channel_messages, task_assigned = excluded.task_assigned, \
             mentions = excluded.mentions, updated_at = excluded.updated_at",
        )
        .bind(user_id.to_string())
        .bind(prefs.direct_messages)
        .bind(prefs.chat_mentions)
        .bind(prefs.thread_replies)
        .bind(prefs.channel_messages)
        .bind(prefs.task_assigned)
        .bind(prefs.mentions)
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    /// Whether the user wants to be notified of this kind of event now: it is on in their
    /// preferences and they did not set "do not disturb".
    async fn wanted(&self, user_id: Id, kind: PushKind) -> Result<bool, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT COALESCE(prefs.{}, 1) AS wanted, COALESCE(status.presence, 'online') AS presence \
             FROM users LEFT JOIN notification_prefs AS prefs ON prefs.user_id = users.id \
             LEFT JOIN user_status AS status ON status.user_id = users.id \
             WHERE users.id = ? AND users.suspended_at IS NULL",
            kind.column()
        ))
        .bind(user_id.to_string())
        .fetch_optional(self.database.pool())
        .await?;
        Ok(row.is_some_and(|row| {
            row.get::<i64, _>("wanted") == 1 && row.get::<String, _>("presence") != "dnd"
        }))
    }

    /// Notifies a user. The tabs they are at get a signal and show it themselves, and nothing
    /// is pushed. Without such a tab, every browser they subscribed with gets a push.
    pub async fn notify(
        &self,
        user_id: Id,
        kind: PushKind,
        notice: &Notice,
    ) -> Result<(), sqlx::Error> {
        if !self.wanted(user_id, kind).await? {
            return Ok(());
        }
        let notice = &notice.clamped();
        if self.hub.signal_user(user_id, true, "notify", notice) {
            return Ok(());
        }
        // Nobody is at an open tab. The open tabs still show it and play the sound, and the
        // other devices get the push.
        self.hub.signal_user(user_id, false, "notify", notice);
        self.deliver(user_id, notice).await.map(|_| ())
    }

    /// Pushes to every browser of the user whose session is still good; returns how many took
    /// it. A subscription that the browser ended is removed.
    pub async fn push(&self, user_id: Id, notice: &Notice) -> Result<usize, sqlx::Error> {
        self.deliver(user_id, &notice.clamped()).await
    }

    /// [`Self::push`] of a notice that was clamped. The browsers get it at the same time: a
    /// user has [`MAX_SUBSCRIPTIONS`] at most.
    async fn deliver(&self, user_id: Id, notice: &Notice) -> Result<usize, sqlx::Error> {
        let now = TimestampMillis::now().as_millis();
        let rows = sqlx::query(
            "SELECT push.id, push.endpoint, push.p256dh, push.auth FROM push_subscriptions AS push \
             JOIN sessions ON sessions.id = push.session_id \
             WHERE push.user_id = ? AND sessions.revoked_at IS NULL \
             AND sessions.idle_expires_at > ? AND sessions.absolute_expires_at > ?",
        )
        .bind(user_id.to_string())
        .bind(now)
        .bind(now)
        .fetch_all(self.database.pool())
        .await?;
        if rows.is_empty() {
            return Ok(0);
        }
        let vapid = self.vapid().await?;
        let payload = serde_json::to_vec(notice).unwrap_or_default();
        if payload.len() > webpush::MAX_PAYLOAD_BYTES {
            // A push service would refuse it. The title and the body are clamped, so this is
            // a URL or a tag that is far too long.
            tracing::warn!(
                bytes = payload.len(),
                "push not sent: the notice is too large"
            );
            return Ok(0);
        }
        let sends = rows.into_iter().filter_map(|row| {
            let id: String = row.get("id");
            let subscription = Subscription {
                id: id.parse().ok()?,
                endpoint: row.get("endpoint"),
                p256dh: row.get("p256dh"),
                auth: row.get("auth"),
            };
            let sender = Arc::clone(&self.inner.sender);
            let (vapid, payload) = (vapid.clone(), payload.clone());
            Some(async move {
                let first = sender.send(vapid.clone(), subscription.clone(), payload.clone());
                let mut result = first.await;
                // A busy push service gets the push once more.
                if matches!(result, Err(SendError::Busy(_))) {
                    tokio::time::sleep(RETRY_AFTER).await;
                    result = sender.send(vapid, subscription, payload).await;
                }
                (id, result)
            })
        });
        let mut sent = 0;
        for (id, result) in futures_util::future::join_all(sends).await {
            match result {
                Ok(()) => {
                    sent += 1;
                    sqlx::query(
                        "UPDATE push_subscriptions SET failures = 0 WHERE id = ? AND failures > 0",
                    )
                    .bind(id)
                    .execute(self.database.pool())
                    .await?;
                }
                Err(SendError::Gone) => {
                    sqlx::query("DELETE FROM push_subscriptions WHERE id = ?")
                        .bind(id)
                        .execute(self.database.pool())
                        .await?;
                }
                // A subscription that fails again and again (a key the push service no longer
                // takes, a service that is gone) is removed; the browser subscribes again when
                // the user turns notifications on.
                Err(SendError::Failed(reason) | SendError::Busy(reason)) => {
                    tracing::warn!(reason, "push not sent");
                    sqlx::query(
                        "UPDATE push_subscriptions SET failures = failures + 1 WHERE id = ?",
                    )
                    .bind(&id)
                    .execute(self.database.pool())
                    .await?;
                    sqlx::query("DELETE FROM push_subscriptions WHERE id = ? AND failures >= ?")
                        .bind(&id)
                        .bind(MAX_FAILURES)
                        .execute(self.database.pool())
                        .await?;
                }
            }
        }
        Ok(sent)
    }

    /// Notifies the members a new chat message is for, after the response went out.
    pub fn chat_message(&self, message: &MessageRecord) {
        if message.deleted {
            return;
        }
        let (service, message) = (self.clone(), message.clone());
        tokio::spawn(async move {
            if let Err(error) = service.chat_message_now(&message).await {
                tracing::warn!(error = %error, "chat push");
            }
        });
    }

    /// As [`Self::chat_message`], done when it returns.
    pub async fn chat_message_now(&self, message: &MessageRecord) -> Result<(), sqlx::Error> {
        let Some(conversation) = sqlx::query(
            "SELECT chat_conversations.kind, chat_conversations.name, chat_conversations.workspace_id, \
             users.display_name AS author, user_avatars.updated_at AS avatar_updated_at \
             FROM chat_conversations JOIN users ON users.id = ? \
             LEFT JOIN user_avatars ON user_avatars.user_id = users.id \
             WHERE chat_conversations.id = ?",
        )
        .bind(message.author_id.to_string())
        .bind(message.conversation_id.to_string())
        .fetch_optional(self.database.pool())
        .await?
        else {
            return Ok(());
        };
        let dm = conversation.get::<String, _>("kind") == "dm";
        let author: String = conversation.get("author");
        let workspace_id: String = conversation.get("workspace_id");
        let thread_only = message.thread_root_id.is_some() && !message.also_in_channel;
        let members = sqlx::query(
            "SELECT members.user_id, members.notify, \
             EXISTS (SELECT 1 FROM chat_message_mentions AS mentions \
               WHERE mentions.message_id = ?1 AND mentions.user_id = members.user_id) AS mentioned, \
             EXISTS (SELECT 1 FROM chat_thread_members AS threads \
               WHERE threads.root_id = ?2 AND threads.user_id = members.user_id \
               AND threads.following = 1) AS following \
             FROM chat_members AS members WHERE members.conversation_id = ?3 AND members.user_id <> ?4",
        )
        .bind(message.id.to_string())
        .bind(message.thread_root_id.map(|id| id.to_string()))
        .bind(message.conversation_id.to_string())
        .bind(message.author_id.to_string())
        .fetch_all(self.database.pool())
        .await?;
        if members.is_empty() {
            return Ok(());
        }
        let online = match workspace_id.parse() {
            Ok(workspace_id) if message.mentions.here => self.hub.online(workspace_id),
            _ => Vec::new(),
        };

        let mut url = format!("/chat/{}", message.conversation_id);
        if let (true, Some(root_id)) = (thread_only, message.thread_root_id) {
            url.push_str(&format!("?thread={root_id}"));
        }
        let notice = |kind: PushKind| Notice {
            title: if dm {
                author.clone()
            } else {
                format!("{author} in #{}", conversation.get::<String, _>("name"))
            },
            body: String::new(),
            url: url.clone(),
            tag: format!("chat:{}", message.conversation_id),
            sound: kind.sound(),
            icon: avatar_url(message.author_id, conversation.get("avatar_updated_at")),
        };
        let body = self.readable(message, &workspace_id).await?;
        let mut notices = Vec::new();
        for member in &members {
            let Ok(user_id) = member.get::<String, _>("user_id").parse::<Id>() else {
                continue;
            };
            let broadcast =
                message.mentions.channel || (message.mentions.here && online.contains(&user_id));
            let kind = chat_kind(ChatAudience {
                dm,
                notify: &member.get::<String, _>("notify"),
                mentioned: member.get::<i64, _>("mentioned") == 1
                    || (!dm && !thread_only && broadcast),
                thread_only,
                following: member.get::<i64, _>("following") == 1,
            });
            if let Some(kind) = kind {
                let notice = Notice {
                    body: body.clone(),
                    ..notice(kind)
                };
                notices.push((user_id, kind, notice));
            }
        }
        // A few members at a time: one slow push service does not hold up a large channel, and
        // a failure for one member does not stop the others.
        futures_util::stream::iter(notices)
            .for_each_concurrent(CONCURRENT_MEMBERS, |(user_id, kind, notice)| async move {
                if let Err(error) = self.notify(user_id, kind, &notice).await {
                    tracing::warn!(error = %error, "chat push");
                }
            })
            .await;
        Ok(())
    }

    /// The body of a message as a notification shows it: names for the mention tokens, one
    /// line, short. A user token gets the name of a member the message mentions, and a channel
    /// token the name of a public channel of the message's workspace. Nothing else is looked
    /// up, so the text cannot name what its author may not see.
    async fn readable(
        &self,
        message: &MessageRecord,
        workspace_id: &str,
    ) -> Result<String, sqlx::Error> {
        let mentioned: HashMap<String, String> = if message.body.contains("<@") {
            sqlx::query(
                "SELECT users.id, users.display_name FROM chat_message_mentions AS mentions \
                 JOIN users ON users.id = mentions.user_id WHERE mentions.message_id = ?",
            )
            .bind(message.id.to_string())
            .fetch_all(self.database.pool())
            .await?
            .into_iter()
            .map(|row| (row.get("id"), row.get("display_name")))
            .collect()
        } else {
            HashMap::new()
        };
        // Each channel is looked up once: the name, or `None` for one that is not shown.
        let mut channels: HashMap<&str, Option<String>> = HashMap::new();
        let mut text = String::new();
        let mut rest = message.body.as_str();
        while let Some(start) = rest.find('<') {
            let (before, token) = rest.split_at(start);
            text.push_str(before);
            let Some(end) = token.find('>') else {
                rest = token;
                break;
            };
            let inner = &token[1..end];
            match inner.split_at_checked(1) {
                Some(("@", id)) => {
                    text.push('@');
                    text.push_str(mentioned.get(id).map_or("someone", String::as_str));
                }
                Some(("#", id)) => {
                    if !channels.contains_key(id) && channels.len() < CHANNEL_LOOKUPS {
                        let name: Option<String> = sqlx::query_scalar(
                            "SELECT name FROM chat_conversations \
                             WHERE id = ? AND workspace_id = ? AND kind = 'public'",
                        )
                        .bind(id)
                        .bind(workspace_id)
                        .fetch_optional(self.database.pool())
                        .await?;
                        channels.insert(id, name);
                    }
                    text.push('#');
                    text.push_str(
                        channels
                            .get(id)
                            .and_then(Option::as_deref)
                            .unwrap_or("channel"),
                    );
                }
                Some(("!", word)) => {
                    text.push('@');
                    text.push_str(word);
                }
                _ => text.push_str(&token[..=end]),
            }
            rest = &token[end + 1..];
        }
        text.push_str(rest);
        let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        Ok(if line.is_empty() && !message.attachments.is_empty() {
            "Sent a file".to_owned()
        } else if line.is_empty() && message.sticker_id.is_some() {
            "Sticker".to_owned()
        } else {
            short(&line, BODY_CHARS)
        })
    }

    /// Notifies the recipients of the inbox notifications that were not pushed yet; returns how
    /// many there were. Chat mentions are marked only: [`Self::chat_message`] did them.
    pub async fn push_inbox(&self) -> Result<usize, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT notifications.id, notifications.recipient_user_id, notifications.kind, \
             notifications.actor_user_id, user_avatars.updated_at AS avatar_updated_at, \
             users.display_name AS actor, tasks.title AS task_title, pages.title AS page_title \
             FROM notifications JOIN users ON users.id = notifications.actor_user_id \
             LEFT JOIN user_avatars ON user_avatars.user_id = users.id \
             LEFT JOIN tasks ON tasks.id = notifications.task_id \
             LEFT JOIN pages ON pages.id = notifications.page_id \
             WHERE notifications.pushed_at IS NULL ORDER BY notifications.id LIMIT 100",
        )
        .fetch_all(self.database.pool())
        .await?;
        for row in &rows {
            let id: String = row.get("id");
            // Marked first: a notification is pushed at most once, also when the push fails.
            sqlx::query("UPDATE notifications SET pushed_at = ? WHERE id = ?")
                .bind(TimestampMillis::now().as_millis())
                .bind(&id)
                .execute(self.database.pool())
                .await?;
            let actor: String = row.get("actor");
            let subject = |column: &str| {
                row.get::<Option<String>, _>(column)
                    .filter(|title| !title.is_empty())
                    .unwrap_or_else(|| "Untitled".to_owned())
            };
            let (kind, title, body) = match row.get::<String, _>("kind").as_str() {
                "task_assigned" => (
                    PushKind::TaskAssigned,
                    format!("{actor} assigned you a task"),
                    subject("task_title"),
                ),
                "comment_mentioned" => (
                    PushKind::Mentions,
                    format!("{actor} mentioned you in a comment"),
                    subject("task_title"),
                ),
                "page_comment_mentioned" => (
                    PushKind::Mentions,
                    format!("{actor} mentioned you in a comment"),
                    subject("page_title"),
                ),
                "page_mentioned" => (
                    PushKind::Mentions,
                    format!("{actor} mentioned you in a page"),
                    subject("page_title"),
                ),
                _ => continue,
            };
            let Ok(user_id) = row.get::<String, _>("recipient_user_id").parse::<Id>() else {
                continue;
            };
            let notice = Notice {
                title,
                body,
                url: "/inbox".to_owned(),
                tag: format!("inbox:{id}"),
                sound: kind.sound(),
                icon: row
                    .get::<String, _>("actor_user_id")
                    .parse()
                    .ok()
                    .and_then(|actor_id| avatar_url(actor_id, row.get("avatar_updated_at"))),
            };
            self.notify(user_id, kind, &notice).await?;
        }
        Ok(rows.len())
    }

    /// Pushes new inbox notifications until `shutdown`.
    pub async fn run_inbox_service(self, shutdown: CancellationToken) -> Result<(), String> {
        loop {
            match self.push_inbox().await {
                // A full batch: there can be more.
                Ok(100) => continue,
                Ok(_) => {}
                Err(error) => tracing::warn!(error = %error, "inbox push"),
            }
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                () = tokio::time::sleep(INBOX_INTERVAL) => {}
            }
        }
    }
}

/// What a chat message is to one member of its conversation.
#[derive(Clone, Copy)]
struct ChatAudience<'a> {
    dm: bool,
    /// The member's notify level of the conversation: `all`, `mentions` or `muted`.
    notify: &'a str,
    /// The message names the member, or all members (`@channel`, `@here`).
    mentioned: bool,
    /// A thread reply that does not show in the main list.
    thread_only: bool,
    /// The member follows the thread of the reply.
    following: bool,
}

/// The kind of event a chat message is for a member; `None` when it does not notify them.
fn chat_kind(audience: ChatAudience<'_>) -> Option<PushKind> {
    match audience {
        ChatAudience {
            notify: "muted", ..
        } => None,
        ChatAudience { dm: true, .. } => Some(PushKind::DirectMessages),
        ChatAudience {
            mentioned: true, ..
        } => Some(PushKind::ChatMentions),
        ChatAudience {
            thread_only: true,
            following,
            ..
        } => following.then_some(PushKind::ThreadReplies),
        ChatAudience { notify: "all", .. } => Some(PushKind::ChannelMessages),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_push_services_of_browser_vendors_are_endpoints() {
        for allowed in [
            "https://fcm.googleapis.com/fcm/send/abc",
            "https://updates.push.services.mozilla.com/wpush/v2/abc",
            "https://db5p.notify.windows.com/w/?token=abc",
            "https://web.push.apple.com/abc",
        ] {
            assert!(allowed_endpoint(allowed), "{allowed}");
        }
        for refused in [
            "http://fcm.googleapis.com/fcm/send/abc",
            "https://fcm.googleapis.com:8443/fcm/send/abc",
            "https://fcm.googleapis.com.evil.example/abc",
            "https://evilpush.apple.com/abc",
            "https://127.0.0.1/abc",
            "https://localhost/abc",
            "not a url",
        ] {
            assert!(!allowed_endpoint(refused), "{refused}");
        }
    }

    #[test]
    fn a_long_title_and_body_are_cut_to_fit_a_push() {
        assert_eq!(short("héllo", 5), "héllo");
        assert_eq!(short("héllo!", 5), "héll…");
        let notice = Notice {
            title: "t".repeat(2000),
            body: "é".repeat(2000),
            url: "/inbox".to_owned(),
            tag: "inbox:1".to_owned(),
            sound: "mention",
            icon: None,
        }
        .clamped();
        assert_eq!(notice.title.chars().count(), TITLE_CHARS);
        assert_eq!(notice.body.chars().count(), BODY_CHARS);
        assert!(notice.body.ends_with('…'));
        assert!(serde_json::to_vec(&notice).unwrap().len() <= webpush::MAX_PAYLOAD_BYTES);
        // Cutting what is short already changes nothing.
        assert_eq!(notice.clamped(), notice);
    }

    #[test]
    fn a_chat_message_notifies_by_the_members_level_and_what_it_is_to_them() {
        let base = ChatAudience {
            dm: false,
            notify: "mentions",
            mentioned: false,
            thread_only: false,
            following: false,
        };
        assert_eq!(chat_kind(base), None);
        assert_eq!(
            chat_kind(ChatAudience {
                notify: "all",
                ..base
            }),
            Some(PushKind::ChannelMessages)
        );
        assert_eq!(
            chat_kind(ChatAudience {
                mentioned: true,
                ..base
            }),
            Some(PushKind::ChatMentions)
        );
        assert_eq!(
            chat_kind(ChatAudience { dm: true, ..base }),
            Some(PushKind::DirectMessages)
        );
        // A reply in a thread: for its followers only, also in a channel set to "all".
        let reply = ChatAudience {
            thread_only: true,
            notify: "all",
            ..base
        };
        assert_eq!(chat_kind(reply), None);
        assert_eq!(
            chat_kind(ChatAudience {
                following: true,
                ..reply
            }),
            Some(PushKind::ThreadReplies)
        );
        assert_eq!(
            chat_kind(ChatAudience {
                mentioned: true,
                ..reply
            }),
            Some(PushKind::ChatMentions)
        );
        // Muted wins over everything.
        for audience in [
            ChatAudience {
                notify: "muted",
                dm: true,
                ..base
            },
            ChatAudience {
                notify: "muted",
                mentioned: true,
                ..base
            },
            ChatAudience {
                notify: "muted",
                thread_only: true,
                following: true,
                ..base
            },
        ] {
            assert_eq!(chat_kind(audience), None);
        }
    }
}
