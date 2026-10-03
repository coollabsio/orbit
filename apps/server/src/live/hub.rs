//! Connections, event numbers, the replay buffer and presence.
//!
//! Locking rule (as in the co-editing hub): the hub's state lock is a plain mutex that is never
//! held across an `await`. A writer that must publish in commit order takes
//! [`LiveHub::write_lock`] *before* it begins its database transaction.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use orbit_platform::{Database, Id, TimestampMillis};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, mpsc};

use crate::repositories::identity::{Presence, UserStatus};

/// Events kept for each workspace, for clients that reconnect.
const BUFFERED_EVENTS: usize = 1000;
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

/// Who an event is for.
#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone)]
pub struct LiveHub {
    inner: Arc<Inner>,
}

struct Inner {
    /// Made when the process starts: a client with another epoch cannot get a replay.
    epoch: String,
    workspaces: Mutex<HashMap<Id, Workspace>>,
    /// The write lock of each workspace that has a write now. They are not in `workspaces`:
    /// a write takes its lock before it knows that the workspace exists.
    write_locks: Mutex<HashMap<Id, Arc<AsyncMutex<()>>>>,
    next_connection: AtomicU64,
    offline_grace: Duration,
    heard_window: Duration,
}

#[derive(Default)]
struct Workspace {
    seq: u64,
    buffer: VecDeque<Buffered>,
    connections: HashMap<u64, Connection>,
    /// Members whose last connection closed a moment ago; still online. The value is that
    /// connection, so only its own timer takes the member offline.
    leaving: HashMap<Id, u64>,
    /// What each online member set for themselves.
    statuses: HashMap<Id, UserStatus>,
}

struct Connection {
    user_id: Id,
    sender: mpsc::Sender<Arc<str>>,
    /// The tab had no input for a while.
    idle: bool,
    /// The window of the tab has been out of focus for a while: the user is not looking at it.
    away: bool,
    /// When the client last sent something (a frame, or the answer to a ping). A connection
    /// whose network died stays silent long before its socket closes.
    heard: Instant,
}

/// What the others see of a member who does not show as offline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Shown {
    user_id: Id,
    /// `online`, `idle` or `dnd`.
    status: &'static str,
    emoji: Option<String>,
    text: Option<String>,
    expires_at: Option<TimestampMillis>,
}

struct Buffered {
    seq: u64,
    recipients: Recipients,
    frame: Arc<str>,
}

/// A new connection: its frames, and what to send first.
pub struct Attached {
    pub id: u64,
    pub frames: mpsc::Receiver<Arc<str>>,
    pub hello: String,
    /// The events the client missed; `None` when the hub cannot tell (the client must resync).
    pub replay: Option<Vec<Arc<str>>>,
}

impl Workspace {
    fn online(&self) -> Vec<Id> {
        let mut users: Vec<Id> = self
            .connections
            .values()
            .map(|connection| connection.user_id)
            .chain(self.leaving.keys().copied())
            .collect();
        users.sort_unstable();
        users.dedup();
        users
    }

    fn is_online(&self, user_id: Id) -> bool {
        self.leaving.contains_key(&user_id)
            || self
                .connections
                .values()
                .any(|connection| connection.user_id == user_id)
    }

    /// Queues a frame for every connection of a recipient. A connection whose queue is full
    /// (or whose socket is gone) is dropped: its socket task sees the closed queue and ends.
    fn deliver(&mut self, recipients: &Recipients, frame: &Arc<str>) {
        self.connections.retain(|_, connection| {
            !recipients.includes(connection.user_id)
                || connection.sender.try_send(Arc::clone(frame)).is_ok()
        });
    }

    fn signal(&mut self, recipients: &Recipients, topic: &str, event: &impl Serialize) {
        let frame: Arc<str> = json!({ "topic": topic, "event": event }).to_string().into();
        self.deliver(recipients, &frame);
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
        let mut tabs = self
            .connections
            .values()
            .filter(|connection| connection.user_id == user_id)
            .peekable();
        // A member is idle when every tab is. One whose last tab closed a moment ago is not.
        let all_idle = tabs.peek().is_some() && tabs.all(|connection| connection.idle);
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

    /// Changes something about a member and tells the workspace, if what the others see of the
    /// member changed.
    fn change(&mut self, user_id: Id, change: impl FnOnce(&mut Self)) {
        let before = self.shown(user_id);
        change(self);
        let after = self.shown(user_id);
        if before != after {
            self.signal(
                &Recipients::Workspace,
                "presence",
                &presence_event(user_id, after),
            );
        }
    }
}

fn presence_event(user_id: Id, shown: Option<Shown>) -> Value {
    let mut event = match shown {
        Some(shown) => json!(shown),
        None => json!({ "user_id": user_id, "status": "offline" }),
    };
    event["type"] = json!("presence");
    event
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
        let inner = database.extension(move || Inner {
            epoch: Id::new_v7().to_string(),
            workspaces: Mutex::new(HashMap::new()),
            write_locks: Mutex::new(HashMap::new()),
            next_connection: AtomicU64::new(1),
            offline_grace,
            heard_window,
        });
        Self { inner }
    }

    fn workspaces(&self) -> std::sync::MutexGuard<'_, HashMap<Id, Workspace>> {
        self.inner
            .workspaces
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Hold this from before a write's transaction until its events are published, so the
    /// events of a workspace go out in commit order.
    pub async fn write_lock(&self, workspace_id: Id) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self
                .inner
                .write_locks
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            // A lock that only this map refers to has no write that holds it or waits for it.
            // Such locks go here, so a write to an id that names no workspace leaves nothing.
            locks.retain(|_, lock| Arc::strong_count(lock) > 1);
            Arc::clone(locks.entry(workspace_id).or_default())
        };
        lock.lock_owned().await
    }

    /// Numbers an event, keeps it for replay and sends it to the recipients who are connected.
    pub fn publish(
        &self,
        workspace_id: Id,
        recipients: &Recipients,
        topic: &str,
        event: &impl Serialize,
    ) {
        let mut workspaces = self.workspaces();
        let workspace = workspaces.entry(workspace_id).or_default();
        workspace.seq += 1;
        let frame: Arc<str> = json!({ "seq": workspace.seq, "topic": topic, "event": event })
            .to_string()
            .into();
        if workspace.buffer.len() == BUFFERED_EVENTS {
            workspace.buffer.pop_front();
        }
        workspace.buffer.push_back(Buffered {
            seq: workspace.seq,
            recipients: recipients.clone(),
            frame: Arc::clone(&frame),
        });
        workspace.deliver(recipients, &frame);
    }

    /// The number of the newest event of a workspace.
    #[must_use]
    pub fn seq(&self, workspace_id: Id) -> u64 {
        self.workspaces()
            .get(&workspace_id)
            .map_or(0, |workspace| workspace.seq)
    }

    /// Sends a signal that is not numbered and not kept (typing).
    pub fn signal(
        &self,
        workspace_id: Id,
        recipients: &Recipients,
        topic: &str,
        event: &impl Serialize,
    ) {
        if let Some(workspace) = self.workspaces().get_mut(&workspace_id) {
            workspace.signal(recipients, topic, event);
        }
    }

    /// The members with a connection (or one that closed a moment ago).
    #[must_use]
    pub fn online(&self, workspace_id: Id) -> Vec<Id> {
        self.workspaces()
            .get(&workspace_id)
            .map(Workspace::online)
            .unwrap_or_default()
    }

    /// The user changed their presence or custom status: every workspace where they are online
    /// shows it, and their own tabs read the status again.
    pub fn set_status(&self, user_id: Id, status: &UserStatus) {
        self.apply_status(user_id, status, true);
    }

    /// As [`Self::set_status`], for a status the user's tabs already read: they are not told to
    /// read it again. The socket uses it when the status changed while it connected.
    pub(crate) fn correct_status(&self, user_id: Id, status: &UserStatus) {
        self.apply_status(user_id, status, false);
    }

    fn apply_status(&self, user_id: Id, status: &UserStatus, tell_own_tabs: bool) {
        for workspace in self.workspaces().values_mut() {
            if !workspace.statuses.contains_key(&user_id) {
                continue;
            }
            workspace.change(user_id, |workspace| {
                workspace.statuses.insert(user_id, status.clone());
            });
            if tell_own_tabs {
                workspace.signal(
                    &Recipients::Users(vec![user_id]),
                    "presence",
                    &json!({ "type": "self" }),
                );
            }
        }
    }

    /// Sends a signal to the tabs of a user in every workspace: with `active_only`, to the tabs
    /// the user is at (input lately, the window in focus or out of it for a moment only, and
    /// the connection heard from lately: one whose network died is no tab the user is at).
    /// `false` when it reached no tab.
    pub fn signal_user(
        &self,
        user_id: Id,
        active_only: bool,
        topic: &str,
        event: &impl Serialize,
    ) -> bool {
        let frame: Arc<str> = json!({ "topic": topic, "event": event }).to_string().into();
        let mut reached = false;
        for workspace in self.workspaces().values_mut() {
            workspace.connections.retain(|_, connection| {
                let active = !connection.idle
                    && !connection.away
                    && connection.heard.elapsed() <= self.inner.heard_window;
                if connection.user_id != user_id || (active_only && !active) {
                    return true;
                }
                let sent = connection.sender.try_send(Arc::clone(&frame)).is_ok();
                reached |= sent;
                sent
            });
        }
        reached
    }

    /// The client of a connection sent something: its network is alive.
    pub fn heard(&self, workspace_id: Id, connection_id: u64) {
        if let Some(connection) = self
            .workspaces()
            .get_mut(&workspace_id)
            .and_then(|workspace| workspace.connections.get_mut(&connection_id))
        {
            connection.heard = Instant::now();
        }
    }

    /// A tab reports that its window has been out of focus for a while, or is in focus again.
    /// It does not change what the others see of the member.
    pub fn set_away(&self, workspace_id: Id, connection_id: u64, away: bool) {
        if let Some(connection) = self
            .workspaces()
            .get_mut(&workspace_id)
            .and_then(|workspace| workspace.connections.get_mut(&connection_id))
        {
            connection.away = away;
        }
    }

    /// A tab reports that it has had no input for a while, or that it has input again.
    pub fn set_idle(&self, workspace_id: Id, connection_id: u64, user_id: Id, idle: bool) {
        if let Some(workspace) = self.workspaces().get_mut(&workspace_id) {
            workspace.change(user_id, |workspace| {
                if let Some(connection) = workspace.connections.get_mut(&connection_id) {
                    connection.idle = idle;
                }
            });
        }
    }

    /// Adds a connection. `status` is what the user set for themselves; `epoch` and `after` are
    /// what the client saw last, if it reconnects.
    pub fn connect(
        &self,
        workspace_id: Id,
        user_id: Id,
        status: UserStatus,
        epoch: Option<&str>,
        after: Option<u64>,
    ) -> Attached {
        let (sender, frames) = mpsc::channel(QUEUED_FRAMES);
        let id = self.inner.next_connection.fetch_add(1, Ordering::Relaxed);
        let mut workspaces = self.workspaces();
        let workspace = workspaces.entry(workspace_id).or_default();
        // The new connection is not there yet: its `hello` tells it about itself.
        workspace.change(user_id, |workspace| {
            workspace.leaving.remove(&user_id);
            workspace.statuses.insert(user_id, status);
        });
        workspace.change(user_id, |workspace| {
            let connection = Connection {
                user_id,
                sender,
                idle: false,
                away: false,
                heard: Instant::now(),
            };
            workspace.connections.insert(id, connection);
        });

        // A replay is complete only if the buffer still starts at or before the first event
        // the client has not seen.
        let replay = after
            .filter(|after| epoch == Some(self.inner.epoch.as_str()) && *after <= workspace.seq)
            .filter(|after| {
                *after == workspace.seq
                    || workspace
                        .buffer
                        .front()
                        .is_some_and(|oldest| oldest.seq <= after + 1)
            })
            .map(|after| {
                workspace
                    .buffer
                    .iter()
                    .filter(|event| event.seq > after && event.recipients.includes(user_id))
                    .map(|event| Arc::clone(&event.frame))
                    .collect()
            });
        let hello = json!({
            "type": "hello",
            "epoch": self.inner.epoch,
            "seq": workspace.seq,
            "presence": workspace.all_shown(),
        })
        .to_string();
        Attached {
            id,
            frames,
            hello,
            replay,
        }
    }

    /// Removes a connection. The member goes offline a moment later, unless it connects again.
    pub fn disconnect(&self, workspace_id: Id, connection_id: u64, user_id: Id) {
        {
            let mut workspaces = self.workspaces();
            let Some(workspace) = workspaces.get_mut(&workspace_id) else {
                return;
            };
            // The tabs that stay can all be idle.
            workspace.change(user_id, |workspace| {
                workspace.connections.remove(&connection_id);
            });
            if workspace.is_online(user_id) {
                return;
            }
            workspace.leaving.insert(user_id, connection_id);
        }
        let hub = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(hub.inner.offline_grace).await;
            let mut workspaces = hub.workspaces();
            if let Some(workspace) = workspaces.get_mut(&workspace_id)
                && workspace.leaving.get(&user_id) == Some(&connection_id)
            {
                workspace.change(user_id, |workspace| {
                    workspace.leaving.remove(&user_id);
                    workspace.statuses.remove(&user_id);
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
            .map(|frame| serde_json::from_str(&frame).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn events_reach_their_recipients_only_and_a_reconnect_gets_what_it_missed() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::of(&database);
        let (workspace, ada, bob) = (Id::new_v7(), Id::new_v7(), Id::new_v7());
        let mut ada_tab = hub.connect(workspace, ada, UserStatus::default(), None, None);
        let mut bob_tab = hub.connect(workspace, bob, UserStatus::default(), None, None);
        assert!(
            ada_tab.replay.is_none(),
            "a first connection has nothing to replay"
        );
        // Ada saw Bob come online; nobody is told about themselves.
        assert_eq!(frames(&mut ada_tab)[0]["event"]["status"], "online");
        assert!(frames(&mut bob_tab).is_empty());

        hub.publish(
            workspace,
            &Recipients::Workspace,
            "chat",
            &json!({ "n": 1 }),
        );
        hub.publish(
            workspace,
            &Recipients::Users(vec![ada]),
            "chat",
            &json!({ "n": 2 }),
        );
        hub.publish(
            workspace,
            &Recipients::WorkspaceExcept(vec![ada]),
            "chat",
            &json!({ "n": 3 }),
        );
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

        // Bob reconnects after event 1: he gets event 3, and never Ada's event 2.
        let hello: Value = serde_json::from_str(&bob_tab.hello).unwrap();
        let epoch = hello["epoch"].as_str().unwrap();
        let again = hub.connect(workspace, bob, UserStatus::default(), Some(epoch), Some(1));
        let replay: Vec<Value> = again
            .replay
            .expect("the buffer reaches back to event 1")
            .iter()
            .map(|frame| serde_json::from_str(frame).unwrap())
            .collect();
        assert_eq!(seen(replay), [(3, 3)]);
        // Another epoch (a restarted server) or a number from the future cannot be replayed.
        assert!(
            hub.connect(
                workspace,
                bob,
                UserStatus::default(),
                Some("other"),
                Some(1)
            )
            .replay
            .is_none()
        );
        assert!(
            hub.connect(workspace, bob, UserStatus::default(), Some(epoch), Some(9))
                .replay
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_reconnect_past_the_buffer_must_resync() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::of(&database);
        let (workspace, ada) = (Id::new_v7(), Id::new_v7());
        let first = hub.connect(workspace, ada, UserStatus::default(), None, None);
        let hello: Value = serde_json::from_str(&first.hello).unwrap();
        let epoch = hello["epoch"].as_str().unwrap().to_owned();
        drop(first);
        for n in 0..=BUFFERED_EVENTS {
            hub.publish(
                workspace,
                &Recipients::Workspace,
                "chat",
                &json!({ "n": n }),
            );
        }
        // Event 1 fell out of the buffer.
        assert!(
            hub.connect(workspace, ada, UserStatus::default(), Some(&epoch), Some(0))
                .replay
                .is_none()
        );
        assert!(
            hub.connect(workspace, ada, UserStatus::default(), Some(&epoch), Some(1))
                .replay
                .is_some()
        );
    }

    #[tokio::test]
    async fn a_member_goes_offline_after_the_delay_unless_it_connects_again() {
        let database = TestDatabase::new().await.unwrap();
        let hub = LiveHub::with_offline_grace(&database, Duration::from_millis(40));
        let (workspace, ada, bob) = (Id::new_v7(), Id::new_v7(), Id::new_v7());
        let mut ada_tab = hub.connect(workspace, ada, UserStatus::default(), None, None);
        let bob_tab = hub.connect(workspace, bob, UserStatus::default(), None, None);
        frames(&mut ada_tab);

        // A reload: the connection closes and a new one opens inside the delay.
        hub.disconnect(workspace, bob_tab.id, bob);
        let bob_tab = hub.connect(workspace, bob, UserStatus::default(), None, None);
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
        let mut ada_tab = hub.connect(workspace, ada, UserStatus::default(), None, None);
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

        // Invisible: Bob connects and nobody is told; the hello does not list him.
        let first = hub.connect(workspace, bob, status(Presence::Invisible), None, None);
        let hello: Value = serde_json::from_str(&first.hello).unwrap();
        assert_eq!(hello["presence"].as_array().unwrap().len(), 1);
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
        let second = hub.connect(workspace, bob, UserStatus::default(), None, None);
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
        let bob_tab = hub.connect(workspace, bob, lunch, None, None);
        let mut ada_tab = hub.connect(workspace, ada, UserStatus::default(), None, None);
        let bob_in = |hello: &str| -> Value {
            let hello: Value = serde_json::from_str(hello).unwrap();
            let mut shown = hello["presence"].as_array().unwrap().iter();
            shown
                .find(|shown| shown["user_id"] == json!(bob))
                .cloned()
                .unwrap()
        };
        assert_eq!(bob_in(&ada_tab.hello)["text"], "Lunch");

        // It ends while Bob is connected: a later hello and a later event do not carry it.
        tokio::time::sleep(Duration::from_millis(450)).await;
        let later = hub.connect(workspace, ada, UserStatus::default(), None, None);
        assert_eq!(
            bob_in(&later.hello),
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
        let mut tab = hub.connect(workspace, ada, UserStatus::default(), None, None);
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

        let first = hub.connect(workspace, ada, UserStatus::default(), None, None);
        hub.disconnect(workspace, first.id, ada);
        tokio::time::sleep(Duration::from_millis(120)).await;
        let second = hub.connect(workspace, ada, UserStatus::default(), None, None);
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
        let locks = || hub.inner.write_locks.lock().unwrap().len();
        // Writes to ids that name no workspace: the locks do not add up.
        for _ in 0..100 {
            drop(hub.write_lock(Id::new_v7()).await);
        }
        assert_eq!(locks(), 1, "only the lock of the last write is left");
        assert!(hub.workspaces().is_empty());

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
        let mut slow = hub.connect(workspace, ada, UserStatus::default(), None, None);
        for n in 0..=QUEUED_FRAMES {
            hub.publish(
                workspace,
                &Recipients::Workspace,
                "chat",
                &json!({ "n": n }),
            );
        }
        assert_eq!(frames(&mut slow).len(), QUEUED_FRAMES);
        // The queue is closed: the socket task ends and the client reconnects.
        assert!(slow.frames.recv().await.is_none());
    }
}
