//! The connections of this server, presence, and the delivery of events to the connections.
//!
//! Numbered events are kept in the database ([`super::events`]). This server delivers the new ones
//! of each workspace that has a connection here, in order ([`LiveHub::deliver_new`]). On Postgres,
//! several servers share the database: through [`crate::cluster`] each tells the others about new
//! events, sends the signals that are not kept (typing, presence, notices), and what it has of
//! every member, so each one shows the same presence.
//!
//! Locking rule (as in the co-editing hub): the hub's state lock is a plain mutex that is never
//! held across an `await`. A writer that must publish in commit order takes
//! [`LiveHub::write_lock`] *before* it begins its database transaction. That orders the writes of
//! one server; two servers that write to the same workspace in the same moment can publish in the
//! other order.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use orbit_platform::{Database, Id, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::AnyPool;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, mpsc};
use tokio_util::sync::CancellationToken;
use tokio_util::task::{TaskTracker, task_tracker::TaskTrackerToken};

use super::events::{self, NewEvent};
use crate::cluster::{Cluster, Message};
use crate::repositories::identity::{Presence, UserStatus};

/// Frames that may wait for one connection. A connection that falls further behind is closed;
/// it reconnects and gets a replay.
const QUEUED_FRAMES: usize = 256;
/// How long a member stays online after the last connection closed, so a page reload does not
/// show as offline and online again.
const OFFLINE_GRACE: Duration = Duration::from_secs(10);
/// How often the socket pings a connection; a live one answers each ping.
pub(super) const HEARTBEAT: Duration = Duration::from_secs(25);
/// How long after a connection was last heard from the user can still be at its tab: a little
/// longer than one heartbeat.
const HEARD_WINDOW: Duration = Duration::from_secs(35);
/// How long a server that stops waits for its connections to send their close frames.
const CLOSE_WAIT: Duration = Duration::from_secs(5);

/// Who an event is for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "to", content = "users", rename_all = "snake_case")]
pub enum Recipients {
    /// Every workspace member.
    Workspace,
    /// Every workspace member but these.
    WorkspaceExcept(Vec<Id>),
    Users(Vec<Id>),
}

impl Recipients {
    #[must_use]
    pub fn includes(&self, user_id: Id) -> bool {
        match self {
            Self::Workspace => true,
            Self::WorkspaceExcept(users) => !users.contains(&user_id),
            Self::Users(users) => users.contains(&user_id),
        }
    }
}

/// A frame for a connection. Events have their number, so a connection can skip the ones its
/// replay already had; signals have none.
#[derive(Clone)]
pub struct Frame {
    pub seq: Option<u64>,
    pub text: Arc<str>,
}

/// What one server tells the others about the live socket.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum LiveMessage {
    /// New events of the workspace are in `live_events`.
    Events {
        workspace_id: Id,
    },
    Signal {
        workspace_id: Id,
        recipients: Recipients,
        frame: String,
    },
    UserSignal {
        user_id: Id,
        active_only: bool,
        frame: String,
    },
    /// What the sending server has of a member; `tabs` is `None` when the member is not online
    /// there any more.
    Tabs {
        workspace_id: Id,
        user_id: Id,
        tabs: Option<Tabs>,
        status: UserStatus,
    },
    Status {
        user_id: Id,
        status: UserStatus,
    },
    /// Asks every server to tell again what it has of every member.
    Sync,
}

/// What one server has of a member in a workspace. A member whose last tab there closed a
/// moment ago has no tabs and is still online.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Tabs {
    tabs: u32,
    idle: u32,
    /// A tab the user is at: input lately, the window in focus, the connection heard from.
    active: bool,
}

#[derive(Clone)]
pub struct LiveHub {
    inner: Arc<Inner>,
    database: Database,
}

struct Inner {
    cluster: Arc<Cluster>,
    workspaces: Mutex<HashMap<Id, Workspace>>,
    /// The write lock of each workspace that has a write now.
    write_locks: KeyedLocks,
    /// One delivery at a time for each workspace, so its events go out in order.
    delivery_locks: KeyedLocks,
    /// The other servers, and when each was last heard from.
    servers: Mutex<HashMap<String, Instant>>,
    next_connection: AtomicU64,
    offline_grace: Duration,
    heard_window: Duration,
    /// Cancelled when the server stops: every connection closes with 1012.
    stopping: CancellationToken,
    /// The connections that are served, so a server that stops can wait for their close frames.
    serving: TaskTracker,
}

/// Async locks by id that leave no entry behind once nobody holds or waits for them.
#[derive(Default)]
struct KeyedLocks(Mutex<HashMap<Id, Arc<AsyncMutex<()>>>>);

impl KeyedLocks {
    async fn lock(&self, id: Id) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            // A lock that only this map refers to has nobody that holds it or waits for it.
            locks.retain(|_, lock| Arc::strong_count(lock) > 1);
            Arc::clone(locks.entry(id).or_default())
        };
        lock.lock_owned().await
    }
}

#[derive(Default)]
struct Workspace {
    connections: HashMap<u64, Connection>,
    /// Members whose last connection here closed a moment ago; still online. The value is that
    /// connection, so only its own timer takes the member offline.
    leaving: HashMap<Id, u64>,
    /// What each online member set for themselves.
    statuses: HashMap<Id, UserStatus>,
    /// What the other servers have of each member, by server.
    remote: HashMap<Id, HashMap<String, Tabs>>,
    /// What this server last told the others it has of each member.
    told: HashMap<Id, Tabs>,
    /// The number of the last event delivered here; `None` while no connection here needs them.
    delivered: Option<u64>,
}

struct Connection {
    user_id: Id,
    sender: mpsc::Sender<Frame>,
    /// The tab had no input for a while.
    idle: bool,
    /// The window of the tab has been out of focus for a while: the user is not looking at it.
    away: bool,
    /// When the client last sent something (a frame, or the answer to a ping). A connection
    /// whose network died stays silent long before its socket closes.
    heard: Instant,
}

impl Connection {
    fn active(&self, heard_window: Duration) -> bool {
        !self.idle && !self.away && self.heard.elapsed() <= heard_window
    }
}

/// What the others see of a member who does not show as offline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Shown {
    user_id: Id,
    /// `online`, `idle` or `dnd`.
    status: &'static str,
    emoji: Option<String>,
    text: Option<String>,
    expires_at: Option<TimestampMillis>,
}

/// A new connection: its frames, and the members who show as online when it connected.
pub struct Attached {
    pub id: u64,
    pub frames: mpsc::Receiver<Frame>,
    pub(super) presence: Vec<Shown>,
}

impl Workspace {
    /// What this server has of a member; `None` when the member is not online here.
    fn local_tabs(&self, user_id: Id, heard_window: Duration) -> Option<Tabs> {
        let mut tabs = Tabs::default();
        for connection in self.connections.values() {
            if connection.user_id == user_id {
                tabs.tabs += 1;
                tabs.idle += u32::from(connection.idle);
                tabs.active |= connection.active(heard_window);
            }
        }
        (tabs.tabs > 0 || self.leaving.contains_key(&user_id)).then_some(tabs)
    }

    fn online_here(&self, user_id: Id) -> bool {
        self.leaving.contains_key(&user_id)
            || self
                .connections
                .values()
                .any(|connection| connection.user_id == user_id)
    }

    fn is_online(&self, user_id: Id) -> bool {
        self.online_here(user_id) || self.remote.contains_key(&user_id)
    }

    fn online(&self) -> Vec<Id> {
        let mut users: Vec<Id> = self
            .connections
            .values()
            .map(|connection| connection.user_id)
            .chain(self.leaving.keys().copied())
            .chain(self.remote.keys().copied())
            .collect();
        users.sort_unstable();
        users.dedup();
        users
    }

    /// Queues a frame for every connection of a recipient. A connection whose queue is full
    /// (or whose socket is gone) is dropped: its socket task sees the closed queue and ends.
    fn deliver(&mut self, recipients: &Recipients, frame: &Frame) {
        self.connections.retain(|_, connection| {
            !recipients.includes(connection.user_id)
                || connection.sender.try_send(frame.clone()).is_ok()
        });
    }

    fn signal(&mut self, recipients: &Recipients, text: &Arc<str>) {
        self.deliver(
            recipients,
            &Frame {
                seq: None,
                text: Arc::clone(text),
            },
        );
    }

    /// What the others see of a member; `None` for offline and for invisible.
    fn shown(&self, user_id: Id) -> Option<Shown> {
        // A custom status that ended is not shown, also when it ended while the member was
        // connected.
        let status = self
            .statuses
            .get(&user_id)?
            .clone()
            .at(TimestampMillis::now());
        let (mut tabs, mut idle) = (0, 0);
        for connection in self.connections.values() {
            if connection.user_id == user_id {
                tabs += 1;
                idle += u32::from(connection.idle);
            }
        }
        for remote in self
            .remote
            .get(&user_id)
            .into_iter()
            .flat_map(HashMap::values)
        {
            tabs += remote.tabs;
            idle += remote.idle;
        }
        // A member is idle when every tab is. One whose last tab closed a moment ago is not.
        let all_idle = tabs > 0 && idle == tabs;
        let shown = match status.presence {
            Presence::Invisible => return None,
            Presence::Dnd => "dnd",
            Presence::Idle => "idle",
            Presence::Online if all_idle => "idle",
            Presence::Online => "online",
        };
        Some(Shown {
            user_id,
            status: shown,
            emoji: status.emoji,
            text: status.text,
            expires_at: status.expires_at,
        })
    }

    /// Everybody who does not show as offline.
    fn all_shown(&self) -> Vec<Shown> {
        let mut shown: Vec<Shown> = self
            .statuses
            .keys()
            .filter_map(|user_id| self.shown(*user_id))
            .collect();
        shown.sort_unstable_by_key(|shown| shown.user_id);
        shown
    }
}

fn frame_text(topic: &str, event: &impl Serialize) -> Arc<str> {
    json!({ "topic": topic, "event": event }).to_string().into()
}

fn presence_event(user_id: Id, shown: Option<Shown>) -> Value {
    let mut event = match shown {
        Some(shown) => json!(shown),
        None => json!({ "user_id": user_id, "status": "offline" }),
    };
    event["type"] = json!("presence");
    event
}

impl Inner {
    fn new(cluster: Arc<Cluster>, offline_grace: Duration, heard_window: Duration) -> Self {
        Self {
            cluster,
            workspaces: Mutex::new(HashMap::new()),
            write_locks: KeyedLocks::default(),
            delivery_locks: KeyedLocks::default(),
            servers: Mutex::new(HashMap::new()),
            next_connection: AtomicU64::new(1),
            offline_grace,
            heard_window,
            stopping: CancellationToken::new(),
            serving: TaskTracker::new(),
        }
    }

    fn workspaces(&self) -> std::sync::MutexGuard<'_, HashMap<Id, Workspace>> {
        self.workspaces
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues a message for the other servers, while the cluster runs.
    fn send(&self, message: LiveMessage) {
        self.cluster.send(Message::Live(message));
    }

    /// Changes something about a member: this server's connections are told when what they see
    /// of the member changed, and the other servers when what this server has of it changed.
    fn change(
        &self,
        workspace_id: Id,
        workspace: &mut Workspace,
        user_id: Id,
        change: impl FnOnce(&mut Workspace),
    ) {
        let before = workspace.shown(user_id);
        change(workspace);
        let after = workspace.shown(user_id);
        if before != after {
            workspace.signal(
                &Recipients::Workspace,
                &frame_text("presence", &presence_event(user_id, after)),
            );
        }
        self.tell_tabs(workspace_id, workspace, user_id);
    }

    fn tell_tabs(&self, workspace_id: Id, workspace: &mut Workspace, user_id: Id) {
        if !self.cluster.running() {
            return;
        }
        let tabs = workspace.local_tabs(user_id, self.heard_window);
        if workspace.told.get(&user_id).copied() == tabs {
            return;
        }
        match tabs {
            Some(tabs) => workspace.told.insert(user_id, tabs),
            None => workspace.told.remove(&user_id),
        };
        self.send(LiveMessage::Tabs {
            workspace_id,
            user_id,
            tabs,
            status: workspace
                .statuses
                .get(&user_id)
                .cloned()
                .unwrap_or_default(),
        });
    }

    fn apply_status(&self, user_id: Id, status: &UserStatus, tell_own_tabs: bool) {
        let mut workspaces = self.workspaces();
        for (workspace_id, workspace) in workspaces.iter_mut() {
            if !workspace.statuses.contains_key(&user_id) {
                continue;
            }
            self.change(*workspace_id, workspace, user_id, |workspace| {
                workspace.statuses.insert(user_id, status.clone());
            });
            if tell_own_tabs {
                workspace.signal(
                    &Recipients::Users(vec![user_id]),
                    &frame_text("presence", &json!({ "type": "self" })),
                );
            }
        }
    }

    fn signal_user_here(&self, user_id: Id, active_only: bool, text: &Arc<str>) -> bool {
        let mut reached = false;
        for workspace in self.workspaces().values_mut() {
            workspace.connections.retain(|_, connection| {
                if connection.user_id != user_id
                    || (active_only && !connection.active(self.heard_window))
                {
                    return true;
                }
                let frame = Frame {
                    seq: None,
                    text: Arc::clone(text),
                };
                let sent = connection.sender.try_send(frame).is_ok();
                reached |= sent;
                sent
            });
        }
        reached
    }

    /// A message from another server; the workspace whose new events to deliver, if any.
    fn receive(&self, server: &str, message: LiveMessage) -> Option<Id> {
        match message {
            LiveMessage::Events { workspace_id } => return Some(workspace_id),
            LiveMessage::Signal {
                workspace_id,
                recipients,
                frame,
            } => {
                if let Some(workspace) = self.workspaces().get_mut(&workspace_id) {
                    workspace.signal(&recipients, &frame.into());
                }
            }
            LiveMessage::UserSignal {
                user_id,
                active_only,
                frame,
            } => {
                self.signal_user_here(user_id, active_only, &frame.into());
            }
            LiveMessage::Tabs {
                workspace_id,
                user_id,
                tabs,
                status,
            } => {
                let mut workspaces = self.workspaces();
                let workspace = workspaces.entry(workspace_id).or_default();
                self.change(workspace_id, workspace, user_id, |workspace| {
                    let servers = workspace.remote.entry(user_id).or_default();
                    match tabs {
                        Some(tabs) => {
                            servers.insert(server.to_owned(), tabs);
                        }
                        None => {
                            servers.remove(server);
                        }
                    }
                    if servers.is_empty() {
                        workspace.remote.remove(&user_id);
                    }
                    if workspace.is_online(user_id) {
                        workspace.statuses.entry(user_id).or_insert(status);
                    } else {
                        workspace.statuses.remove(&user_id);
                    }
                });
            }
            LiveMessage::Status { user_id, status } => self.apply_status(user_id, &status, true),
            LiveMessage::Sync => self.tell_all_again(),
        }
        None
    }

    /// Tells the other servers again everything this server has of every member (a server
    /// joined, or this one lost messages).
    fn tell_all_again(&self) {
        let mut workspaces = self.workspaces();
        for (workspace_id, workspace) in workspaces.iter_mut() {
            workspace.told.clear();
            let users: Vec<Id> = workspace
                .connections
                .values()
                .map(|connection| connection.user_id)
                .chain(workspace.leaving.keys().copied())
                .collect();
            for user_id in users {
                self.tell_tabs(*workspace_id, workspace, user_id);
            }
        }
    }

    /// Every few seconds on Postgres: tells the others what changed with time (a tab that went
    /// silent is no tab the user is at), and forgets the servers that stopped.
    fn tick(&self, server_timeout: Duration) {
        let gone: Vec<String> = {
            let mut servers = self.servers.lock().unwrap_or_else(PoisonError::into_inner);
            let gone = servers
                .iter()
                .filter(|(_, heard)| heard.elapsed() > server_timeout)
                .map(|(server, _)| server.clone())
                .collect::<Vec<_>>();
            for server in &gone {
                servers.remove(server);
            }
            gone
        };
        let mut workspaces = self.workspaces();
        for (workspace_id, workspace) in workspaces.iter_mut() {
            let users: Vec<Id> = workspace
                .connections
                .values()
                .map(|connection| connection.user_id)
                .chain(workspace.remote.keys().copied())
                .collect();
            for user_id in users {
                self.change(*workspace_id, workspace, user_id, |workspace| {
                    if let Some(servers) = workspace.remote.get_mut(&user_id) {
                        servers.retain(|server, _| !gone.contains(server));
                        if servers.is_empty() {
                            workspace.remote.remove(&user_id);
                        }
                    }
                    if !workspace.is_online(user_id) {
                        workspace.statuses.remove(&user_id);
                    }
                });
            }
        }
    }

    /// The workspaces whose events this server delivers.
    fn delivering(&self) -> Vec<Id> {
        self.workspaces()
            .iter()
            .filter(|(_, workspace)| workspace.delivered.is_some())
            .map(|(workspace_id, _)| *workspace_id)
            .collect()
    }
}

/// Delivers the events of a workspace that this server has not delivered yet, in order.
async fn deliver_new(inner: &Inner, pool: &AnyPool, workspace_id: Id) {
    let _one = inner.delivery_locks.lock(workspace_id).await;
    let Some(after) = inner
        .workspaces()
        .get(&workspace_id)
        .and_then(|workspace| workspace.delivered)
    else {
        return;
    };
    let stored = match events::after(pool, workspace_id, after).await {
        Ok(stored) => stored,
        Err(error) => {
            tracing::warn!(error = %error, "live events");
            return;
        }
    };
    let mut workspaces = inner.workspaces();
    let Some(workspace) = workspaces.get_mut(&workspace_id) else {
        return;
    };
    let Some(mut delivered) = workspace.delivered else {
        return;
    };
    for event in stored {
        if event.seq <= delivered {
            continue;
        }
        let frame = Frame {
            seq: Some(event.seq),
            text: event.frame,
        };
        workspace.deliver(&event.recipients, &frame);
        delivered = event.seq;
    }
    workspace.delivered = Some(delivered);
}

impl LiveHub {
    /// The hub of this database (one for each open database; made on first use).
    #[must_use]
    pub fn of(database: &Database) -> Self {
        Self::with(database, OFFLINE_GRACE, HEARD_WINDOW)
    }

    /// As [`Self::of`], with another offline delay if the database has no hub yet (tests).
    #[must_use]
    pub fn with_offline_grace(database: &Database, offline_grace: Duration) -> Self {
        Self::with(database, offline_grace, HEARD_WINDOW)
    }

    /// As [`Self::of`], with another time for which a silent connection still counts as active,
    /// if the database has no hub yet (tests).
    #[must_use]
    pub fn with_heard_window(database: &Database, heard_window: Duration) -> Self {
        Self::with(database, OFFLINE_GRACE, heard_window)
    }

    fn with(database: &Database, offline_grace: Duration, heard_window: Duration) -> Self {
        let cluster = Cluster::of(database);
        Self {
            inner: database.extension(|| Inner::new(cluster, offline_grace, heard_window)),
            database: database.clone(),
        }
    }

    /// The hub of this database, if anything made one.
    #[must_use]
    pub fn existing(database: &Database) -> Option<Self> {
        database.existing_extension::<Inner>().map(|inner| Self {
            inner,
            database: database.clone(),
        })
    }

    /// Cancelled when the server stops; the connection then closes with 1012.
    pub(super) fn stopping(&self) -> CancellationToken {
        self.inner.stopping.clone()
    }

    /// Held while a connection is served, until its close frame is sent.
    pub(super) fn serving(&self) -> TaskTrackerToken {
        self.inner.serving.token()
    }

    /// Server shutdown: closes every connection with 1012 (service restart), so the clients
    /// reconnect at once to another server or to this one when it is back, and waits a moment
    /// for the close frames to go out.
    pub async fn shutdown(&self) {
        self.inner.stopping.cancel();
        self.inner.serving.close();
        if tokio::time::timeout(CLOSE_WAIT, self.inner.serving.wait())
            .await
            .is_err()
        {
            tracing::warn!("live sockets did not close in time");
        }
    }

    /// The cluster started on this server: it tells the others what it has, and asks them to
    /// tell theirs.
    pub(crate) fn joined_cluster(&self) {
        self.inner.tell_all_again();
        self.inner.send(LiveMessage::Sync);
    }

    /// Another server was heard from: it is alive.
    pub(crate) fn heard_server(&self, server: &str) {
        self.inner
            .servers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(server.to_owned(), Instant::now());
    }

    /// A message from another server.
    pub(crate) async fn receive(&self, server: &str, message: LiveMessage) {
        if let Some(workspace_id) = self.inner.receive(server, message) {
            self.deliver_new(workspace_id).await;
        }
    }

    /// Delivers what this server may have missed.
    pub(crate) async fn catch_up(&self) {
        for workspace_id in self.inner.delivering() {
            self.deliver_new(workspace_id).await;
        }
    }

    /// Every few seconds while the cluster runs: tells the others what changed with time,
    /// forgets the servers not heard from for `server_timeout`, and delivers what it missed.
    pub(crate) async fn tick(&self, server_timeout: Duration) {
        self.inner.tick(server_timeout);
        self.catch_up().await;
    }

    /// Hold this from before a write's transaction until its events are published, so the
    /// events of a workspace go out in commit order.
    pub async fn write_lock(&self, workspace_id: Id) -> OwnedMutexGuard<()> {
        self.inner.write_locks.lock(workspace_id).await
    }

    /// Numbers and keeps events, and sends them to the recipients who are connected to any
    /// server. Returns the number of the last one; with no events, the newest number.
    pub async fn publish(&self, workspace_id: Id, events: &[NewEvent]) -> Result<u64, sqlx::Error> {
        let seq = events::append(&self.database, workspace_id, events).await?;
        if !events.is_empty() {
            self.inner.send(LiveMessage::Events { workspace_id });
            self.deliver_new(workspace_id).await;
        }
        Ok(seq)
    }

    /// Delivers the events of a workspace that this server has not delivered yet.
    pub async fn deliver_new(&self, workspace_id: Id) {
        deliver_new(&self.inner, self.database.pool(), workspace_id).await;
    }

    /// A connection of the workspace starts after event `newest`: from now on this server
    /// delivers the events after it (if it does not already), and the ones that are there.
    pub async fn start_delivery(&self, workspace_id: Id, newest: u64) {
        if let Some(workspace) = self.inner.workspaces().get_mut(&workspace_id)
            && !workspace.connections.is_empty()
            && workspace.delivered.is_none()
        {
            workspace.delivered = Some(newest);
        }
        self.deliver_new(workspace_id).await;
    }

    /// Sends a signal that is not numbered and not kept (typing).
    pub fn signal(
        &self,
        workspace_id: Id,
        recipients: &Recipients,
        topic: &str,
        event: &impl Serialize,
    ) {
        let text = frame_text(topic, event);
        if let Some(workspace) = self.inner.workspaces().get_mut(&workspace_id) {
            workspace.signal(recipients, &text);
        }
        self.inner.send(LiveMessage::Signal {
            workspace_id,
            recipients: recipients.clone(),
            frame: text.to_string(),
        });
    }

    /// The members with a connection to any server (or one that closed a moment ago).
    #[must_use]
    pub fn online(&self, workspace_id: Id) -> Vec<Id> {
        self.inner
            .workspaces()
            .get(&workspace_id)
            .map(Workspace::online)
            .unwrap_or_default()
    }

    /// The user changed their presence or custom status: every workspace where they are online
    /// shows it, and their own tabs read the status again.
    pub fn set_status(&self, user_id: Id, status: &UserStatus) {
        self.inner.apply_status(user_id, status, true);
        self.inner.send(LiveMessage::Status {
            user_id,
            status: status.clone(),
        });
    }

    /// As [`Self::set_status`], for a status the user's tabs already read: they are not told to
    /// read it again. The socket uses it when the status changed while it connected.
    pub(crate) fn correct_status(&self, user_id: Id, status: &UserStatus) {
        self.inner.apply_status(user_id, status, false);
    }

    /// Sends a signal to the tabs of a user in every workspace, on every server: with
    /// `active_only`, to the tabs the user is at (input lately, the window in focus or out of it
    /// for a moment only, and the connection heard from lately: one whose network died is no
    /// tab the user is at). `false` when it reached no tab.
    pub fn signal_user(
        &self,
        user_id: Id,
        active_only: bool,
        topic: &str,
        event: &impl Serialize,
    ) -> bool {
        let text = frame_text(topic, event);
        let mut reached = self.inner.signal_user_here(user_id, active_only, &text);
        let elsewhere = self.inner.workspaces().values().any(|workspace| {
            workspace
                .remote
                .get(&user_id)
                .is_some_and(|servers| servers.values().any(|tabs| tabs.active || !active_only))
        });
        if elsewhere {
            reached = true;
            self.inner.send(LiveMessage::UserSignal {
                user_id,
                active_only,
                frame: text.to_string(),
            });
        }
        reached
    }

    /// The client of a connection sent something: its network is alive.
    pub fn heard(&self, workspace_id: Id, connection_id: u64) {
        if let Some(connection) = self
            .inner
            .workspaces()
            .get_mut(&workspace_id)
            .and_then(|workspace| workspace.connections.get_mut(&connection_id))
        {
            connection.heard = Instant::now();
        }
    }

    /// A tab reports that its window has been out of focus for a while, or is in focus again.
    /// It does not change what the others see of the member.
    pub fn set_away(&self, workspace_id: Id, connection_id: u64, user_id: Id, away: bool) {
        if let Some(workspace) = self.inner.workspaces().get_mut(&workspace_id) {
            self.inner
                .change(workspace_id, workspace, user_id, |workspace| {
                    if let Some(connection) = workspace.connections.get_mut(&connection_id) {
                        connection.away = away;
                    }
                });
        }
    }

    /// A tab reports that it has had no input for a while, or that it has input again.
    pub fn set_idle(&self, workspace_id: Id, connection_id: u64, user_id: Id, idle: bool) {
        if let Some(workspace) = self.inner.workspaces().get_mut(&workspace_id) {
            self.inner
                .change(workspace_id, workspace, user_id, |workspace| {
                    if let Some(connection) = workspace.connections.get_mut(&connection_id) {
                        connection.idle = idle;
                    }
                });
        }
    }

    /// Adds a connection. `status` is what the user set for themselves.
    pub fn connect(&self, workspace_id: Id, user_id: Id, status: UserStatus) -> Attached {
        let (sender, frames) = mpsc::channel(QUEUED_FRAMES);
        let id = self.inner.next_connection.fetch_add(1, Ordering::Relaxed);
        let mut workspaces = self.inner.workspaces();
        let workspace = workspaces.entry(workspace_id).or_default();
        // The new connection is not there yet: it learns about itself from the presence list.
        self.inner
            .change(workspace_id, workspace, user_id, |workspace| {
                workspace.leaving.remove(&user_id);
                workspace.statuses.insert(user_id, status);
            });
        self.inner
            .change(workspace_id, workspace, user_id, |workspace| {
                let connection = Connection {
                    user_id,
                    sender,
                    idle: false,
                    away: false,
                    heard: Instant::now(),
                };
                workspace.connections.insert(id, connection);
            });
        Attached {
            id,
            frames,
            presence: workspace.all_shown(),
        }
    }

    /// Removes a connection. The member goes offline a moment later, unless it connects again.
    pub fn disconnect(&self, workspace_id: Id, connection_id: u64, user_id: Id) {
        {
            let mut workspaces = self.inner.workspaces();
            let Some(workspace) = workspaces.get_mut(&workspace_id) else {
                return;
            };
            // The tabs that stay can all be idle.
            self.inner
                .change(workspace_id, workspace, user_id, |workspace| {
                    workspace.connections.remove(&connection_id);
                    if workspace.connections.is_empty() {
                        workspace.delivered = None;
                    }
                });
            if workspace.online_here(user_id) {
                return;
            }
            self.inner
                .change(workspace_id, workspace, user_id, |workspace| {
                    workspace.leaving.insert(user_id, connection_id);
                });
        }
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            tokio::time::sleep(inner.offline_grace).await;
            let mut workspaces = inner.workspaces();
            if let Some(workspace) = workspaces.get_mut(&workspace_id)
                && workspace.leaving.get(&user_id) == Some(&connection_id)
            {
                inner.change(workspace_id, workspace, user_id, |workspace| {
                    workspace.leaving.remove(&user_id);
                    if !workspace.is_online(user_id) {
                        workspace.statuses.remove(&user_id);
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_platform::TestDatabase;

    fn frames(attached: &mut Attached) -> Vec<Value> {
        std::iter::from_fn(|| attached.frames.try_recv().ok())
            .map(|frame| serde_json::from_str(&frame.text).unwrap())
            .collect()
    }

    fn event(recipients: Recipients, n: u64) -> NewEvent {
        NewEvent {
            recipients,
            topic: "chat",
            event: json!({ "n": n }),
        }
    }

    async fn page_collab_generation(database: &Database) {
        // Migrations made the generation the epoch is read from.
        assert!(!events::epoch(database.pool()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn events_reach_their_recipients_only_and_are_kept_in_order() {
        let database = TestDatabase::new().await.unwrap();
        page_collab_generation(&database).await;
        let hub = LiveHub::of(&database);
        let (workspace, ada, bob) = (Id::new_v7(), Id::new_v7(), Id::new_v7());
        let mut ada_tab = hub.connect(workspace, ada, UserStatus::default());
        let mut bob_tab = hub.connect(workspace, bob, UserStatus::default());
        hub.start_delivery(workspace, 0).await;
        // Ada saw Bob come online; nobody is told about themselves.
        assert_eq!(frames(&mut ada_tab)[0]["event"]["status"], "online");
        assert!(frames(&mut bob_tab).is_empty());

        let last = hub
            .publish(
                workspace,
                &[
                    event(Recipients::Workspace, 1),
                    event(Recipients::Users(vec![ada]), 2),
                    event(Recipients::WorkspaceExcept(vec![ada]), 3),
                ],
            )
            .await
            .unwrap();
        assert_eq!(last, 3);
        let seen = |frames: Vec<Value>| -> Vec<(u64, u64)> {
            frames
                .iter()
                .map(|frame| {
                    (
                        frame["seq"].as_u64().unwrap(),
                        frame["event"]["n"].as_u64().unwrap(),
                    )
                })
                .collect()
        };
        assert_eq!(seen(frames(&mut ada_tab)), [(1, 1), (2, 2)]);
        assert_eq!(seen(frames(&mut bob_tab)), [(1, 1), (3, 3)]);

        // A reconnect after event 1 reads the rest from the database, Ada's event too: the
        // socket filters it.
        let kept = events::after(database.pool(), workspace, 1).await.unwrap();
        assert_eq!(
            kept.iter().map(|event| event.seq).collect::<Vec<_>>(),
            [2, 3]
        );
        assert!(!kept[0].recipients.includes(bob));
        assert_eq!(
            events::range(database.pool(), workspace).await.unwrap(),
            (1, 3)
        );
        // No events: the newest number.
        assert_eq!(hub.publish(workspace, &[]).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn only_the_newest_events_are_kept() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::of(&database);
        let workspace = Id::new_v7();
        let batch: Vec<NewEvent> = (0..=events::BUFFERED_EVENTS)
            .map(|n| event(Recipients::Workspace, u64::try_from(n).unwrap()))
            .collect();
        hub.publish(workspace, &batch).await.unwrap();
        // Event 1 fell out of the buffer.
        assert_eq!(
            events::range(database.pool(), workspace).await.unwrap(),
            (2, 1001)
        );
    }

    #[tokio::test]
    async fn a_member_goes_offline_after_the_delay_unless_it_connects_again() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::with_offline_grace(&database, Duration::from_millis(40));
        let (workspace, ada, bob) = (Id::new_v7(), Id::new_v7(), Id::new_v7());
        let mut ada_tab = hub.connect(workspace, ada, UserStatus::default());
        let bob_tab = hub.connect(workspace, bob, UserStatus::default());
        frames(&mut ada_tab);

        // A reload: the connection closes and a new one opens inside the delay.
        hub.disconnect(workspace, bob_tab.id, bob);
        let bob_tab = hub.connect(workspace, bob, UserStatus::default());
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(frames(&mut ada_tab).is_empty());
        assert_eq!(hub.online(workspace).len(), 2);

        hub.disconnect(workspace, bob_tab.id, bob);
        assert_eq!(
            hub.online(workspace).len(),
            2,
            "still online during the delay"
        );
        tokio::time::sleep(Duration::from_millis(80)).await;
        let offline = frames(&mut ada_tab);
        assert_eq!(offline.len(), 1);
        assert_eq!(
            offline[0]["event"],
            json!({ "type": "presence", "user_id": bob, "status": "offline" })
        );
        assert_eq!(hub.online(workspace), [ada]);
    }

    #[tokio::test]
    async fn the_others_see_the_status_a_member_set_and_idle_only_when_every_tab_is() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::of(&database);
        let (workspace, ada, bob) = (Id::new_v7(), Id::new_v7(), Id::new_v7());
        let mut ada_tab = hub.connect(workspace, ada, UserStatus::default());
        let status = |presence| UserStatus {
            presence,
            text: Some("Lunch".to_owned()),
            ..UserStatus::default()
        };
        let seen = |tab: &mut Attached| -> Vec<Value> {
            frames(tab)
                .into_iter()
                .map(|frame| frame["event"].clone())
                .filter(|event| event["type"] == "presence")
                .collect()
        };

        // Invisible: Bob connects and nobody is told; the presence list does not have him.
        let first = hub.connect(workspace, bob, status(Presence::Invisible));
        assert_eq!(first.presence.len(), 1);
        assert!(seen(&mut ada_tab).is_empty());
        // `@here` and the per-member events still count him as connected.
        assert_eq!(hub.online(workspace).len(), 2);

        hub.set_status(bob, &status(Presence::Dnd));
        let events = seen(&mut ada_tab);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["status"], "dnd");
        assert_eq!(events[0]["text"], "Lunch");
        // The same status again changes nothing for the others.
        hub.set_status(bob, &status(Presence::Dnd));
        assert!(seen(&mut ada_tab).is_empty());

        // Online with two tabs: idle only when both are, online again when one has input, and
        // idle again when the tab with input closes.
        hub.set_status(bob, &UserStatus::default());
        let second = hub.connect(workspace, bob, UserStatus::default());
        assert_eq!(seen(&mut ada_tab)[0]["status"], "online");
        hub.set_idle(workspace, first.id, bob, true);
        assert!(seen(&mut ada_tab).is_empty());
        hub.set_idle(workspace, second.id, bob, true);
        assert_eq!(seen(&mut ada_tab)[0]["status"], "idle");
        hub.set_idle(workspace, second.id, bob, false);
        assert_eq!(seen(&mut ada_tab)[0]["status"], "online");
        hub.disconnect(workspace, second.id, bob);
        assert_eq!(seen(&mut ada_tab)[0]["status"], "idle");

        // A status change reaches the member's own tabs as a request to read it again.
        let mut first = first;
        let own: Vec<Value> = frames(&mut first)
            .into_iter()
            .map(|frame| frame["event"].clone())
            .filter(|event| event["type"] == "self")
            .collect();
        assert_eq!(own.len(), 3);
    }

    #[tokio::test]
    async fn a_custom_status_that_ended_is_not_shown() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::of(&database);
        let (workspace, ada, bob) = (Id::new_v7(), Id::new_v7(), Id::new_v7());
        let lunch = UserStatus {
            emoji: Some("🍜".to_owned()),
            text: Some("Lunch".to_owned()),
            expires_at: Some(TimestampMillis::from_millis(
                TimestampMillis::now().as_millis() + 300,
            )),
            ..UserStatus::default()
        };
        let bob_tab = hub.connect(workspace, bob, lunch);
        let mut ada_tab = hub.connect(workspace, ada, UserStatus::default());
        let bob_in = |presence: &[Shown]| -> Value {
            presence
                .iter()
                .find(|shown| shown.user_id == bob)
                .map(|shown| json!(shown))
                .unwrap()
        };
        assert_eq!(bob_in(&ada_tab.presence)["text"], "Lunch");

        // It ends while Bob is connected: a later presence list and a later event do not carry
        // it.
        tokio::time::sleep(Duration::from_millis(450)).await;
        let later = hub.connect(workspace, ada, UserStatus::default());
        assert_eq!(
            bob_in(&later.presence),
            json!({ "user_id": bob, "status": "online", "emoji": null, "text": null, "expires_at": null })
        );
        hub.set_idle(workspace, bob_tab.id, bob, true);
        let events = frames(&mut ada_tab);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0]["event"],
            json!({ "type": "presence", "user_id": bob, "status": "idle", "emoji": null, "text": null,
                "expires_at": null })
        );
    }

    #[tokio::test]
    async fn a_connection_that_is_silent_is_not_a_tab_the_user_is_at() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::with_heard_window(&database, Duration::from_millis(60));
        let (workspace, ada) = (Id::new_v7(), Id::new_v7());
        let mut tab = hub.connect(workspace, ada, UserStatus::default());
        let at_a_tab = || hub.signal_user(ada, true, "notify", &json!({}));
        assert!(at_a_tab(), "a new connection was heard from");

        // The network died: no frame and no answer to a ping.
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(!at_a_tab());
        assert_eq!(frames(&mut tab).len(), 1);
        // It still gets what is for every tab.
        assert!(hub.signal_user(ada, false, "notify", &json!({})));

        hub.heard(workspace, tab.id);
        assert!(at_a_tab());
        assert_eq!(frames(&mut tab).len(), 2);
    }

    #[tokio::test]
    async fn an_old_offline_timer_does_not_end_a_later_delay() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::with_offline_grace(&database, Duration::from_millis(200));
        let (workspace, ada) = (Id::new_v7(), Id::new_v7());

        let first = hub.connect(workspace, ada, UserStatus::default());
        hub.disconnect(workspace, first.id, ada);
        tokio::time::sleep(Duration::from_millis(120)).await;
        let second = hub.connect(workspace, ada, UserStatus::default());
        hub.disconnect(workspace, second.id, ada);

        // The first timer ends here; the second delay has only started.
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(hub.online(workspace), [ada]);
        tokio::time::sleep(Duration::from_millis(160)).await;
        assert!(hub.online(workspace).is_empty());
    }

    #[tokio::test]
    async fn a_write_lock_leaves_no_entry_and_still_keeps_the_order() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::of(&database);
        let locks = || hub.inner.write_locks.0.lock().unwrap().len();
        // Writes to ids that name no workspace: the locks do not add up.
        for _ in 0..100 {
            drop(hub.write_lock(Id::new_v7()).await);
        }
        assert_eq!(locks(), 1, "only the lock of the last write is left");
        assert!(hub.inner.workspaces().is_empty());

        // A held lock stays through the writes of other workspaces: the next write of its
        // workspace waits for it.
        let workspace = Id::new_v7();
        let held = hub.write_lock(workspace).await;
        drop(hub.write_lock(Id::new_v7()).await);
        let waiting = tokio::spawn({
            let hub = hub.clone();
            async move { drop(hub.write_lock(workspace).await) }
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        drop(hub.write_lock(Id::new_v7()).await);
        assert!(!waiting.is_finished(), "the first write still has the lock");
        drop(held);
        waiting.await.unwrap();
        drop(hub.write_lock(Id::new_v7()).await);
        assert_eq!(locks(), 1);
    }

    #[tokio::test]
    async fn a_connection_that_does_not_read_is_dropped() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::of(&database);
        let (workspace, ada) = (Id::new_v7(), Id::new_v7());
        let mut slow = hub.connect(workspace, ada, UserStatus::default());
        hub.start_delivery(workspace, 0).await;
        let batch: Vec<NewEvent> = (0..=QUEUED_FRAMES)
            .map(|n| event(Recipients::Workspace, u64::try_from(n).unwrap()))
            .collect();
        hub.publish(workspace, &batch).await.unwrap();
        assert_eq!(frames(&mut slow).len(), QUEUED_FRAMES);
        // The queue is closed: the socket task ends and the client reconnects.
        assert!(slow.frames.recv().await.is_none());
    }
}
