//! Rooms: one live Yjs document per open page or task description ([`DocKey`]), shared by every
//! connection editing it. Pages and tasks have the same table pair each (`page_collab_*`,
//! `task_collab_*`); the SQL below names the page tables and [`DocKind::sql`] maps it per kind.
//!
//! Locking rule: a room's state lock is always taken *before* a database transaction is begun,
//! never while one is open, so room locks and SQLite's single writer cannot deadlock.
//!
//! Persistence: client updates are applied in memory, broadcast, and queued; the queue is written
//! as one merged row of `page_collab_updates` after 150 ms or 64 updates. The log is folded into
//! `page_collab_docs.snapshot` every 500 updates or 1 MiB of log, and when a room is evicted. The
//! JSON projection (`pages.content_json` / `content_text`) runs 2 s after the last change, at
//! least every 10 s while people type, on eviction and on demand ([`CollabHub::flush_page`]).
//!
//! Several servers (Postgres): each has its own room for a page, and the rooms converge. Small
//! updates and cursors go to the other servers at once ([`crate::cluster`]); the update log is
//! the truth. Before a server writes its batch, a server-side replacement or a compaction, it
//! reads the rows (and a newer snapshot) that the others wrote, in the same serialized write
//! transaction ([`CollabHub::catch_up`]), so a snapshot always holds every row it replaces.
//! Applying an update twice changes nothing, so a room may get one both ways.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use orbit_platform::{Backend, Database, Flag, Id, Row, TimestampMillis};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Any, AnyConnection, AnyPool, Transaction};
use thiserror::Error;
use tokio::sync::{Mutex, OwnedMutexGuard, broadcast};
use yrs::encoding::read::Cursor;
use yrs::sync::{Awareness, AwarenessUpdate, Message, MessageReader, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{ClientID, ReadTxn, StateVector, Transact, Update, WriteTxn};

use super::blocknote::{self, FRAGMENT};
use super::close;
use super::sanitize::{repair_document_for, sanitize_blocks_for};
use super::{DocKey, DocKind, LinkPolicy, kind_page, kind_task};
use crate::cluster::{Cluster, Message as ClusterMessage};

/// Limits and timings of the co-editing hub.
#[derive(Clone, Debug)]
pub struct CollabConfig {
    /// Largest accepted WebSocket frame (the page API's body limit).
    pub max_frame_bytes: usize,
    /// Largest encoded document state.
    pub max_doc_bytes: usize,
    pub max_connections_per_page: usize,
    pub max_connections_per_user: usize,
    pub max_connections: usize,
    /// Token buckets per connection (capacity = one second's worth).
    pub sync_messages_per_second: u32,
    pub awareness_messages_per_second: u32,
    /// Write-behind batch: flushed when its oldest update is this old...
    pub flush_after: Duration,
    /// ...or holds this many updates.
    pub flush_max_updates: usize,
    /// Compaction thresholds for the update log.
    pub compact_updates: usize,
    pub compact_bytes: usize,
    /// Projection: this long after the last change...
    pub project_idle: Duration,
    /// ...or at least this often while changes keep coming.
    pub project_max_delay: Duration,
    /// How long a room without connections stays loaded (tab switches).
    pub room_ttl: Duration,
    /// Rough memory budget for loaded documents (estimated as 17x the encoded state).
    pub memory_budget_bytes: usize,
    /// At most one `page.updated` audit event per page per this interval for co-editing.
    pub audit_interval: Duration,
    pub ping_interval: Duration,
    /// Periodic access re-check of every connection (session expiry and the like).
    pub recheck_interval: Duration,
    /// Maintenance tick (flushes, projections, eviction).
    pub tick: Duration,
}

impl Default for CollabConfig {
    fn default() -> Self {
        Self {
            max_frame_bytes: 1024 * 1024,
            max_doc_bytes: 8 * 1024 * 1024,
            max_connections_per_page: 50,
            max_connections_per_user: 20,
            max_connections: 2_000,
            sync_messages_per_second: 200,
            awareness_messages_per_second: 30,
            flush_after: Duration::from_millis(150),
            flush_max_updates: 64,
            compact_updates: 500,
            compact_bytes: 1024 * 1024,
            project_idle: Duration::from_secs(2),
            project_max_delay: Duration::from_secs(10),
            room_ttl: Duration::from_secs(30),
            memory_budget_bytes: 256 * 1024 * 1024,
            audit_interval: Duration::from_secs(10 * 60),
            ping_interval: Duration::from_secs(20),
            recheck_interval: Duration::from_secs(15),
            tick: Duration::from_millis(50),
        }
    }
}

/// Loaded documents take about this many times their encoded size in memory (spike measurement).
const MEMORY_FACTOR: usize = 17;
/// Awareness states a single connection may own (one per tab is normal).
const MAX_AWARENESS_CLIENTS_PER_CONNECTION: usize = 4;
/// Largest awareness state (cursor, user) accepted.
const MAX_AWARENESS_STATE_BYTES: usize = 16 * 1024;
/// Cursor colors (Tailwind 600: readable on dark and light backgrounds). Each page gives every
/// person their own while one is free (see [`room_color`]); neighbours differ in hue, so the
/// next free color is easy to tell apart too.
const USER_COLORS: [&str; 9] = [
    "#e11d48", "#2563eb", "#d97706", "#7c3aed", "#059669", "#ea580c", "#0891b2", "#c026d3",
    "#65a30d",
];

#[derive(Debug, Error)]
pub enum CollabError {
    #[error("document was not found")]
    NotFound,
    #[error("co-editing is overloaded")]
    Overloaded,
    #[error("collaborative document is invalid: {0}")]
    Corrupt(String),
    #[error("collaborative storage is unavailable")]
    Unavailable(#[from] sqlx::Error),
    /// Another server reset the document (new epoch): the room must start over.
    #[error("collaborative document was reset")]
    Reset,
}

/// What one server tells the others about co-editing.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum CollabMessage {
    /// A client's document update as the sending server applied it (base64; small ones only:
    /// the others read larger ones from the log).
    Update {
        key: DocKey,
        epoch: String,
        data: String,
        user_id: Id,
    },
    /// A y-sync awareness frame (cursors), base64.
    Awareness { key: DocKey, data: String },
    /// The sending server wrote rows of the page's update log, or a new snapshot.
    Logged { key: DocKey },
    /// Close the page's room and its connections with this code (document reset, lock change).
    Close { key: DocKey, code: u16 },
    /// The connections of the workspace (all with `None`) check their access again.
    Revalidate { workspace_id: Option<Id> },
}

/// Base64 of at most this many bytes goes to the other servers in a message.
const MAX_MESSAGE_DATA: usize = 6_000;

fn close_reason(code: u16) -> &'static str {
    match code {
        close::RESET => "document reset",
        close::LOCK_CHANGED => "page lock changed",
        _ => "document reloaded",
    }
}

/// Something every connection of a room receives.
#[derive(Clone, Debug)]
pub(crate) enum Outbound {
    /// A binary frame; `except` is the sending connection.
    Frame { except: Option<u64>, data: Bytes },
    /// Close with this code.
    Close { code: u16, reason: &'static str },
    /// Re-run the access check (page moved, trashed, member removed...).
    Revalidate,
}

pub(crate) struct Room {
    pub(crate) key: DocKey,
    workspace_id: OnceLock<Id>,
    state: Arc<Mutex<RoomState>>,
    pub(crate) tx: broadcast::Sender<Outbound>,
    /// Lock-free copies for the maintenance scan.
    connections: AtomicUsize,
    approx_bytes: AtomicUsize,
    idle_since_ms: AtomicU64,
}

impl Room {
    /// The document's content policy (its workspace is known once the room is loaded).
    fn policy(&self) -> LinkPolicy {
        LinkPolicy {
            kind: self.key.kind,
            workspace_id: self.workspace_id.get().copied().unwrap_or(self.key.id),
            doc_id: self.key.id,
        }
    }
}

pub(crate) struct RoomState {
    loaded: bool,
    /// The page has a stored collaborative document (`page_collab_docs` row).
    initialized: bool,
    closed: bool,
    /// The page is locked: content updates from clients are ignored (see [`CollabLock`]).
    locked: bool,
    pub(crate) awareness: Awareness,
    /// Deltas captured by the document observer during the current transaction(s).
    captured: Arc<StdMutex<Vec<Vec<u8>>>>,
    pub(crate) epoch: String,
    /// Write-behind batch.
    pending: Vec<Vec<u8>>,
    pending_updates: usize,
    pending_since: Option<Instant>,
    /// Log rows since the snapshot.
    last_seq: i64,
    log_updates: usize,
    log_bytes: usize,
    projected_seq: i64,
    /// Set while the document has changes not yet written to `pages.content_json`.
    unprojected_since: Option<Instant>,
    last_change: Instant,
    last_editor: Option<Id>,
    state_bytes: usize,
    connections: usize,
    /// Awareness client id -> owning connection.
    awareness_owners: HashMap<u64, u64>,
    /// Connection -> its user, for connections that published awareness (they hold a color).
    color_holders: HashMap<u64, Id>,
    /// User -> index into [`USER_COLORS`], while one of their connections is in the room.
    user_colors: HashMap<Id, usize>,
}

impl RoomState {
    fn new() -> Self {
        let captured = Arc::new(StdMutex::new(Vec::new()));
        let awareness = Awareness::new(observed_doc(&captured));
        Self {
            loaded: false,
            initialized: false,
            closed: false,
            locked: false,
            awareness,
            captured,
            epoch: String::new(),
            pending: Vec::new(),
            pending_updates: 0,
            pending_since: None,
            last_seq: 0,
            log_updates: 0,
            log_bytes: 0,
            projected_seq: 0,
            unprojected_since: None,
            last_change: Instant::now(),
            last_editor: None,
            state_bytes: 0,
            connections: 0,
            awareness_owners: HashMap::new(),
            color_holders: HashMap::new(),
            user_colors: HashMap::new(),
        }
    }

    fn take_captured(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.captured.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn full_state(&self) -> Vec<u8> {
        self.awareness
            .doc()
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    /// Replaces the document with a fresh one (reset), keeping the observer.
    fn reset_doc(&mut self) {
        self.awareness = Awareness::new(observed_doc(&self.captured));
        self.take_captured();
    }

    fn mark_changed(&mut self, editor: Option<Id>) {
        let now = Instant::now();
        self.last_change = now;
        self.unprojected_since.get_or_insert(now);
        if editor.is_some() {
            self.last_editor = editor;
        }
    }
}

fn observed_doc(captured: &Arc<StdMutex<Vec<Vec<u8>>>>) -> yrs::Doc {
    let doc = blocknote::new_doc(None);
    let sink = Arc::clone(captured);
    doc.observe_update_v1("orbit-collab", move |_txn, event| {
        sink.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.update.clone());
    })
    .expect("a fresh document accepts an update observer");
    doc
}

/// The co-editing hub of one database (see [`CollabHub::of`]).
#[derive(Clone)]
pub struct CollabHub {
    inner: Arc<HubInner>,
}

pub(crate) struct HubInner {
    pool: AnyPool,
    backend: Backend,
    cluster: Arc<Cluster>,
    config: CollabConfig,
    rooms: StdMutex<HashMap<DocKey, Arc<Room>>>,
    users: StdMutex<HashMap<Id, usize>>,
    connections: AtomicUsize,
    next_connection: AtomicU64,
    ticker_started: AtomicBool,
    stopping: AtomicBool,
    started: Instant,
}

/// A connection admitted to a room.
pub(crate) struct Joined {
    pub(crate) room: Arc<Room>,
    pub(crate) connection: u64,
    pub(crate) rx: broadcast::Receiver<Outbound>,
    pub(crate) greeting: Vec<Vec<u8>>,
    pub(crate) epoch: String,
}

/// Who is on the other end of a connection (for awareness and the update log).
#[derive(Clone, Debug)]
pub(crate) struct Peer {
    pub(crate) user_id: Id,
    pub(crate) name: String,
}

/// What handling one inbound frame produced.
pub(crate) struct Handled {
    pub(crate) replies: Vec<Vec<u8>>,
    pub(crate) close: Option<(u16, &'static str)>,
}

impl CollabHub {
    /// A write transaction serialized like `Database::immediate_transaction` (the hub keeps the
    /// pool, not the `Database`, which owns the hub).
    async fn write_transaction(&self) -> Result<Transaction<'static, Any>, sqlx::Error> {
        match self.inner.backend {
            Backend::Sqlite => self.inner.pool.begin_with("BEGIN IMMEDIATE").await,
            Backend::Postgres => {
                let mut tx = self.inner.pool.begin().await?;
                // Same advisory lock as `orbit_platform::Database::immediate_transaction`.
                sqlx::query("SELECT 1 FROM pg_advisory_xact_lock(8030594652542121842)")
                    .execute(&mut *tx)
                    .await?;
                Ok(tx)
            }
        }
    }

    /// The hub of this database (one per open database; created on first use).
    #[must_use]
    pub fn of(database: &Database) -> Self {
        Self::install(database, CollabConfig::default())
    }

    /// Like [`Self::of`], but creates the hub with `config` if the database has none yet (tests).
    pub fn install(database: &Database, config: CollabConfig) -> Self {
        let pool = database.pool().clone();
        let backend = database.backend();
        let cluster = Cluster::of(database);
        let inner = database.extension(move || HubInner {
            pool,
            backend,
            cluster,
            config,
            rooms: StdMutex::new(HashMap::new()),
            users: StdMutex::new(HashMap::new()),
            connections: AtomicUsize::new(0),
            next_connection: AtomicU64::new(1),
            ticker_started: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            started: Instant::now(),
        });
        Self { inner }
    }

    /// The hub of this database if one was ever used.
    #[must_use]
    pub fn existing(database: &Database) -> Option<Self> {
        database
            .existing_extension::<HubInner>()
            .map(|inner| Self { inner })
    }

    #[must_use]
    pub fn config(&self) -> &CollabConfig {
        &self.inner.config
    }

    /// Tells the other servers, while the cluster runs.
    fn tell(&self, message: CollabMessage) {
        self.inner.cluster.send(ClusterMessage::Collab(message));
    }

    /// Other servers may write the same documents (Postgres).
    fn shared(&self) -> bool {
        self.inner.backend == Backend::Postgres
    }

    /// Reads what other servers wrote to the page's log since this room's last row (and their
    /// newer snapshot, if they compacted past it) into the room, and sends it to this server's
    /// connections. On Postgres only; inside a write transaction it sees every committed row.
    async fn catch_up(
        &self,
        connection: &mut AnyConnection,
        room: &Room,
        state: &mut RoomState,
    ) -> Result<(), CollabError> {
        if !self.shared() || !state.initialized {
            return Ok(());
        }
        let kind = room.key.kind;
        let page = room.key.id.to_string();
        let row = sqlx::query(
            kind.sql("SELECT snapshot_seq, epoch FROM page_collab_docs WHERE page_id = $1"),
        )
        .bind(&page)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(CollabError::NotFound)?;
        if row.get::<String, _>("epoch") != state.epoch {
            return Err(CollabError::Reset);
        }
        let snapshot_seq: i64 = row.get("snapshot_seq");
        let snapshot: Option<Vec<u8>> = if snapshot_seq > state.last_seq {
            sqlx::query_scalar(kind.sql("SELECT snapshot FROM page_collab_docs WHERE page_id = $1"))
                .bind(&page)
                .fetch_optional(&mut *connection)
                .await?
        } else {
            None
        };
        let log = sqlx::query(kind.sql(
            "SELECT seq, data, user_id FROM page_collab_updates WHERE page_id = $1 AND seq > $2 \
             ORDER BY seq",
        ))
        .bind(&page)
        .bind(snapshot_seq.max(state.last_seq))
        .fetch_all(&mut *connection)
        .await?;
        if snapshot.is_none() && log.is_empty() {
            return Ok(());
        }
        {
            let doc = state.awareness.doc();
            let mut txn = doc.transact_mut();
            if let Some(snapshot) = &snapshot {
                apply(&mut txn, snapshot)?;
            }
            for entry in &log {
                apply(&mut txn, &entry.get::<Vec<u8>, _>("data"))?;
            }
        }
        if snapshot.is_some() {
            state.last_seq = snapshot_seq;
            state.log_updates = 0;
            state.log_bytes = 0;
        }
        if let Some(last) = log.last() {
            state.last_seq = last.get("seq");
            state.log_updates += log.len();
            state.log_bytes += log
                .iter()
                .map(|entry| entry.get::<Vec<u8>, _>("data").len())
                .sum::<usize>();
            if let Some(editor) = last
                .get::<Option<String>, _>("user_id")
                .and_then(|id| id.parse().ok())
            {
                state.last_editor = Some(editor);
            }
        }
        let deltas = state.take_captured();
        for delta in &deltas {
            state.state_bytes += delta.len();
            broadcast_update(room, None, delta);
        }
        if !deltas.is_empty() {
            state.mark_changed(None);
            self.set_bytes(room, state);
        }
        Ok(())
    }

    /// [`Self::catch_up`] outside a transaction; a reset document closes the room.
    async fn catch_up_logged(&self, room: &Arc<Room>, state: &mut RoomState) {
        let result = match self.inner.pool.acquire().await {
            Ok(mut connection) => self.catch_up(&mut connection, room, state).await,
            Err(error) => Err(error.into()),
        };
        match result {
            Ok(()) => {}
            Err(CollabError::Reset) => self.close_room(room, state, close::RESET),
            Err(error) => {
                tracing::warn!(doc = %room.key, error = %error, "co-editing catch-up failed");
            }
        }
    }

    fn close_room(&self, room: &Arc<Room>, state: &mut RoomState, code: u16) {
        state.closed = true;
        let _ = room.tx.send(Outbound::Close {
            code,
            reason: close_reason(code),
        });
        self.forget(room);
    }

    /// A message from another server.
    pub(crate) async fn receive(&self, message: CollabMessage) {
        let key = match &message {
            CollabMessage::Revalidate { workspace_id } => {
                self.revalidate_here(*workspace_id);
                return;
            }
            CollabMessage::Update { key, .. }
            | CollabMessage::Awareness { key, .. }
            | CollabMessage::Logged { key }
            | CollabMessage::Close { key, .. } => *key,
        };
        let Some(room) = self.rooms().get(&key).cloned() else {
            return;
        };
        let mut state = Arc::clone(&room.state).lock_owned().await;
        if state.closed || !state.initialized {
            return;
        }
        match message {
            CollabMessage::Update {
                epoch,
                data,
                user_id,
                ..
            } => {
                let Ok(update) = BASE64.decode(data) else {
                    return;
                };
                if epoch != state.epoch {
                    return;
                }
                let applied = {
                    let doc = state.awareness.doc();
                    let mut txn = doc.transact_mut();
                    apply(&mut txn, &update)
                };
                let deltas = state.take_captured();
                if applied.is_err() {
                    return;
                }
                for delta in &deltas {
                    state.state_bytes += delta.len();
                    broadcast_update(&room, None, delta);
                }
                if !deltas.is_empty() {
                    state.mark_changed(Some(user_id));
                    self.set_bytes(&room, &state);
                }
            }
            CollabMessage::Awareness { data, .. } => {
                let Ok(frame) = BASE64.decode(data) else {
                    return;
                };
                if let Ok(Message::Awareness(update)) = Message::decode_v1(&frame)
                    && state.awareness.apply_update(update).is_ok()
                {
                    let _ = room.tx.send(Outbound::Frame {
                        except: None,
                        data: Bytes::from(frame),
                    });
                }
            }
            CollabMessage::Logged { .. } => self.catch_up_logged(&room, &mut state).await,
            CollabMessage::Close { code, .. } => self.close_room(&room, &mut state, code),
            CollabMessage::Revalidate { .. } => {}
        }
    }

    /// Reads what the other servers wrote into every loaded room (messages may have been lost).
    pub(crate) async fn catch_up_all(&self) {
        let rooms: Vec<Arc<Room>> = self.rooms().values().cloned().collect();
        for room in rooms {
            let mut state = Arc::clone(&room.state).lock_owned().await;
            if !state.closed && state.initialized {
                self.catch_up_logged(&room, &mut state).await;
            }
        }
    }

    /// Sends a client's update (if small) or awareness frame to the other servers.
    fn share(&self, message: CollabMessage) {
        let data = match &message {
            CollabMessage::Update { data, .. } | CollabMessage::Awareness { data, .. } => data,
            _ => "",
        };
        if data.len() <= MAX_MESSAGE_DATA {
            self.tell(message);
        }
    }

    fn rooms(&self) -> std::sync::MutexGuard<'_, HashMap<DocKey, Arc<Room>>> {
        self.inner
            .rooms
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.inner.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Number of loaded rooms and open connections (diagnostics, tests).
    #[must_use]
    pub fn stats(&self) -> (usize, usize) {
        (
            self.rooms().len(),
            self.inner.connections.load(Ordering::Relaxed),
        )
    }

    fn start_ticker(&self) {
        if self.inner.ticker_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak: Weak<HubInner> = Arc::downgrade(&self.inner);
        let tick = self.inner.config.tick;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let Some(inner) = weak.upgrade() else { return };
                let hub = CollabHub { inner };
                if hub.inner.stopping.load(Ordering::SeqCst) {
                    return;
                }
                hub.maintain().await;
            }
        });
    }

    /// The document's room, loaded (and, with `init`, initialized from its stored content), locked.
    async fn room(
        &self,
        key: DocKey,
        init: bool,
    ) -> Result<(Arc<Room>, OwnedMutexGuard<RoomState>), CollabError> {
        self.start_ticker();
        for _ in 0..8 {
            let (room, fresh) = {
                let mut rooms = self.rooms();
                match rooms.get(&key) {
                    Some(room) => (Arc::clone(room), None),
                    None => {
                        let (tx, _) = broadcast::channel(1024);
                        let room = Arc::new(Room {
                            key,
                            workspace_id: OnceLock::new(),
                            state: Arc::new(Mutex::new(RoomState::new())),
                            tx,
                            connections: AtomicUsize::new(0),
                            approx_bytes: AtomicUsize::new(0),
                            idle_since_ms: AtomicU64::new(self.now_ms()),
                        });
                        let guard = Arc::clone(&room.state)
                            .try_lock_owned()
                            .expect("a new room's lock is free");
                        rooms.insert(key, Arc::clone(&room));
                        (room, Some(guard))
                    }
                }
            };
            let mut state = match fresh {
                Some(guard) => guard,
                None => Arc::clone(&room.state).lock_owned().await,
            };
            if state.closed {
                self.forget(&room);
                continue;
            }
            if !state.loaded
                && let Err(error) = self.load(&room, &mut state).await
            {
                state.closed = true;
                self.forget(&room);
                return Err(error);
            }
            if init
                && !state.initialized
                && let Err(error) = self.initialize(&room, &mut state).await
            {
                state.closed = true;
                self.forget(&room);
                return Err(error);
            }
            return Ok((room, state));
        }
        Err(CollabError::Overloaded)
    }

    fn forget(&self, room: &Arc<Room>) {
        let mut rooms = self.rooms();
        if rooms
            .get(&room.key)
            .is_some_and(|live| Arc::ptr_eq(live, room))
        {
            rooms.remove(&room.key);
        }
    }

    /// Reads the stored document (snapshot + log) into the room.
    async fn load(&self, room: &Room, state: &mut RoomState) -> Result<(), CollabError> {
        let pool = &self.inner.pool;
        let kind = room.key.kind;
        let (workspace_id, locked) = match kind {
            DocKind::Page => kind_page::meta(pool, room.key.id).await?,
            DocKind::Task => kind_task::meta(pool, room.key.id).await?,
        }
        .ok_or(CollabError::NotFound)?;
        let _ = room.workspace_id.set(workspace_id);
        state.locked = locked;
        let row = sqlx::query(kind.sql(
            "SELECT snapshot, snapshot_seq, epoch, converter_version, projected_seq \
             FROM page_collab_docs WHERE page_id = $1",
        ))
        .bind(room.key.id.to_string())
        .fetch_optional(pool)
        .await?;
        state.loaded = true;
        let Some(row) = row else {
            state.initialized = false;
            state.epoch = generation(pool).await?;
            return Ok(());
        };
        let snapshot_seq: i64 = row.get("snapshot_seq");
        let log = sqlx::query(kind.sql(
            "SELECT seq, data, user_id FROM page_collab_updates WHERE page_id = $1 AND seq > $2 \
             ORDER BY seq",
        ))
        .bind(room.key.id.to_string())
        .bind(snapshot_seq)
        .fetch_all(pool)
        .await?;
        {
            let doc = state.awareness.doc();
            let mut txn = doc.transact_mut();
            let snapshot: Vec<u8> = row.get("snapshot");
            apply(&mut txn, &snapshot)?;
            for entry in &log {
                apply(&mut txn, &entry.get::<Vec<u8>, _>("data"))?;
            }
        }
        state.take_captured();
        state.initialized = true;
        state.epoch = row.get("epoch");
        state.last_seq = log
            .last()
            .map_or(snapshot_seq, |entry| entry.get::<i64, _>("seq"));
        state.log_updates = log.len();
        state.log_bytes = log
            .iter()
            .map(|entry| entry.get::<Vec<u8>, _>("data").len())
            .sum();
        state.projected_seq = row.get("projected_seq");
        state.last_editor = log
            .iter()
            .rev()
            .find_map(|entry| entry.get::<Option<String>, _>("user_id"))
            .and_then(|id| id.parse().ok());
        if state.last_seq > state.projected_seq {
            // Changes that were persisted but never projected (crash): project soon.
            state.mark_changed(None);
        }
        state.state_bytes = state.full_state().len();
        self.set_bytes(room, state);
        if row.get::<i64, _>("converter_version") != blocknote::converter_version() {
            self.rebuild(room, state).await?;
        }
        // Another server may have compacted between the reads above.
        let mut connection = self.inner.pool.acquire().await?;
        self.catch_up(&mut connection, room, state).await
    }

    /// First open of a document: builds it from the stored content (`pages.content_json`, a
    /// task's description).
    async fn initialize(&self, room: &Room, state: &mut RoomState) -> Result<(), CollabError> {
        let pool = &self.inner.pool;
        let blocks = self.content(room).await?;
        state.reset_doc();
        write_document(state, &blocks, &room.policy());
        state.take_captured();
        let snapshot = state.full_state();
        let workspace_id = *room.workspace_id.get().ok_or(CollabError::NotFound)?;
        let now = TimestampMillis::now().as_millis();
        let inserted = sqlx::query(room.key.kind.sql(
            "INSERT INTO page_collab_docs (page_id, workspace_id, snapshot, snapshot_seq, epoch, \
             converter_version, projected_seq, created_at, updated_at) \
             VALUES ($1, $2, $3, 0, $4, $5, 0, $6, $7) ON CONFLICT (page_id) DO NOTHING",
        ))
        .bind(room.key.id.to_string())
        .bind(workspace_id.to_string())
        .bind(&snapshot)
        .bind(&state.epoch)
        .bind(blocknote::converter_version())
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?
        .rows_affected();
        if inserted == 0 {
            // Another server opened the page first: use its document.
            state.reset_doc();
            return self.load(room, state).await;
        }
        state.initialized = true;
        state.last_seq = 0;
        state.projected_seq = 0;
        state.log_updates = 0;
        state.log_bytes = 0;
        state.unprojected_since = None;
        state.state_bytes = snapshot.len();
        self.set_bytes(room, state);
        Ok(())
    }

    /// The stored document was built by another converter version: project it with this one
    /// (if it has unprojected changes), then rebuild it from the JSON under a new epoch, so
    /// connected clients (old epoch) reset.
    async fn rebuild(&self, room: &Room, state: &mut RoomState) -> Result<(), CollabError> {
        if state.unprojected_since.is_some() {
            self.persist(room, state, true, false).await?;
        }
        let blocks = self.content(room).await?;
        state.reset_doc();
        write_document(state, &blocks, &room.policy());
        state.take_captured();
        let snapshot = state.full_state();
        let epoch = random_epoch();
        let mut tx = self.write_transaction().await?;
        sqlx::query(room.key.kind.sql(
            "UPDATE page_collab_docs SET snapshot = $1, snapshot_seq = $2, epoch = $3, \
             converter_version = $4, projected_seq = $5, updated_at = $6 WHERE page_id = $7",
        ))
        .bind(&snapshot)
        .bind(state.last_seq)
        .bind(&epoch)
        .bind(blocknote::converter_version())
        .bind(state.last_seq)
        .bind(TimestampMillis::now().as_millis())
        .bind(room.key.id.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            room.key
                .kind
                .sql("DELETE FROM page_collab_updates WHERE page_id = $1 AND seq <= $2"),
        )
        .bind(room.key.id.to_string())
        .bind(state.last_seq)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        state.epoch = epoch;
        state.projected_seq = state.last_seq;
        state.log_updates = 0;
        state.log_bytes = 0;
        state.unprojected_since = None;
        state.state_bytes = snapshot.len();
        self.set_bytes(room, state);
        let _ = room.tx.send(Outbound::Close {
            code: close::RESET,
            reason: "document reset",
        });
        self.tell(CollabMessage::Close {
            key: room.key,
            code: close::RESET,
        });
        Ok(())
    }

    /// The stored content a new document is built from.
    async fn content(&self, room: &Room) -> Result<Vec<Value>, CollabError> {
        match room.key.kind {
            DocKind::Page => kind_page::content(&self.inner.pool, room.key.id).await,
            DocKind::Task => {
                let workspace_id = *room.workspace_id.get().ok_or(CollabError::NotFound)?;
                kind_task::content(&self.inner.pool, room.key.id, workspace_id).await
            }
        }
    }

    fn set_bytes(&self, room: &Room, state: &RoomState) {
        room.approx_bytes
            .store(state.state_bytes * MEMORY_FACTOR, Ordering::Relaxed);
    }

    fn memory_in_use(&self) -> usize {
        self.rooms()
            .values()
            .map(|room| room.approx_bytes.load(Ordering::Relaxed))
            .sum()
    }

    /// Writes the pending batch and, as asked, the projection and a compaction, in one
    /// transaction. On failure the room keeps its state and retries on the next tick.
    async fn persist(
        &self,
        room: &Room,
        state: &mut RoomState,
        project: bool,
        compact: bool,
    ) -> Result<(), CollabError> {
        if !state.initialized {
            state.pending.clear();
            return Ok(());
        }
        let key = room.key;
        let workspace_id = *room.workspace_id.get().ok_or(CollabError::NotFound)?;
        let mut tx = self.write_transaction().await?;
        // First what the other servers wrote: the projection and the snapshot must have it.
        if let Err(error) = self.catch_up(&mut tx, room, state).await {
            let _ = tx.rollback().await;
            return Err(error);
        }
        let project = project && state.unprojected_since.is_some();
        if project && repair(state, &room.policy()) {
            // The repair is an ordinary update: persisted with the batch and broadcast.
            self.queue_repair(room, state);
        }
        let compact = compact && (state.log_updates > 0 || !state.pending.is_empty());
        if state.pending.is_empty() && !project && !compact {
            let _ = tx.rollback().await;
            return Ok(());
        }
        let now = TimestampMillis::now();
        let merged = if state.pending.is_empty() {
            None
        } else {
            match yrs::merge_updates_v1(&state.pending) {
                Ok(merged) => Some(merged),
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(CollabError::Corrupt(error.to_string()));
                }
            }
        };
        let blocks = project.then(|| {
            sanitize_blocks_for(
                &blocknote::doc_to_blocks(state.awareness.doc()),
                &room.policy(),
            )
        });
        let snapshot = compact.then(|| state.full_state());
        let result: Result<(i64, bool), CollabError> = async {
            let mut seq = state.last_seq;
            if let Some(data) = &merged {
                seq = append(&mut tx, key, workspace_id, data, state.last_editor, now).await?;
            }
            let mut audited = false;
            if let Some(blocks) = &blocks {
                let audited_at: Option<i64> = sqlx::query_scalar(
                    key.kind
                        .sql("SELECT audited_at FROM page_collab_docs WHERE page_id = $1"),
                )
                .bind(key.id.to_string())
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
                let interval =
                    i64::try_from(self.inner.config.audit_interval.as_millis()).unwrap_or(i64::MAX);
                let audit = state.last_editor.is_some()
                    && audited_at.is_none_or(|at| now.as_millis().saturating_sub(at) >= interval);
                let changed = match key.kind {
                    DocKind::Page => {
                        kind_page::project(&mut tx, key.id, blocks, state.last_editor, audit, now)
                            .await?
                    }
                    DocKind::Task => {
                        kind_task::project(
                            &mut tx,
                            key.id,
                            workspace_id,
                            blocks,
                            state.last_editor,
                            audit,
                            now,
                        )
                        .await?
                    }
                };
                audited = changed && audit;
                sqlx::query(key.kind.sql(
                    "UPDATE page_collab_docs SET projected_seq = $1, \
                     audited_at = CASE WHEN $2 THEN $3 ELSE audited_at END, updated_at = $4 \
                     WHERE page_id = $5",
                ))
                .bind(seq)
                .bind(audited)
                .bind(now.as_millis())
                .bind(now.as_millis())
                .bind(key.id.to_string())
                .execute(&mut *tx)
                .await?;
            }
            if let Some(snapshot) = &snapshot {
                compact_log(&mut tx, key, snapshot, seq, now).await?;
            }
            Ok((seq, audited))
        }
        .await;
        let (seq, _) = match result {
            Ok(done) => done,
            Err(error) => {
                let _ = tx.rollback().await;
                return Err(error);
            }
        };
        tx.commit().await?;
        if merged.is_some() {
            state.log_updates += state.pending_updates.max(1);
            state.log_bytes += merged.as_ref().map_or(0, Vec::len);
        }
        state.pending.clear();
        state.pending_updates = 0;
        state.pending_since = None;
        state.last_seq = seq;
        if blocks.is_some() {
            state.projected_seq = seq;
            state.unprojected_since = None;
        }
        if let Some(snapshot) = snapshot {
            state.log_updates = 0;
            state.log_bytes = 0;
            state.state_bytes = snapshot.len();
            self.set_bytes(room, state);
        }
        if merged.is_some() || compact {
            self.tell(CollabMessage::Logged { key });
        }
        Ok(())
    }

    /// Queues and broadcasts the deltas of a server-side repair transaction.
    fn queue_repair(&self, room: &Room, state: &mut RoomState) {
        for delta in state.take_captured() {
            state.state_bytes += delta.len();
            broadcast_update(room, None, &delta);
            state.pending.push(delta);
            state.pending_updates += 1;
            state.pending_since.get_or_insert_with(Instant::now);
        }
    }

    /// Persists (and projects) after an error-tolerant attempt; logs failures.
    async fn persist_logged(
        &self,
        room: &Room,
        state: &mut RoomState,
        project: bool,
        compact: bool,
    ) -> bool {
        match self.persist(room, state, project, compact).await {
            Ok(()) => true,
            Err(CollabError::NotFound) => {
                // The page is gone (purged): nothing to keep.
                state.pending.clear();
                state.unprojected_since = None;
                state.closed = true;
                let _ = room.tx.send(Outbound::Close {
                    code: close::GONE,
                    reason: "document deleted",
                });
                false
            }
            Err(CollabError::Reset) => {
                // Another server reset the document; this room's edits since are dropped, as
                // its clients' are (they reset too).
                state.pending.clear();
                state.unprojected_since = None;
                state.closed = true;
                let _ = room.tx.send(Outbound::Close {
                    code: close::RESET,
                    reason: "document reset",
                });
                false
            }
            Err(error) => {
                tracing::warn!(doc = %room.key, error = %error, "co-editing persistence failed");
                false
            }
        }
    }

    fn compaction_due(&self, state: &RoomState) -> bool {
        state.log_updates + state.pending_updates >= self.inner.config.compact_updates
            || state.log_bytes >= self.inner.config.compact_bytes
    }

    /// One maintenance pass: flush batches, project, compact, evict idle rooms.
    async fn maintain(&self) {
        let config = &self.inner.config;
        let rooms: Vec<Arc<Room>> = self.rooms().values().cloned().collect();
        let now = Instant::now();
        let now_ms = self.now_ms();
        for room in rooms {
            let Ok(mut state) = Arc::clone(&room.state).try_lock_owned() else {
                continue;
            };
            if state.closed {
                self.forget(&room);
                continue;
            }
            let idle_for = Duration::from_millis(
                now_ms.saturating_sub(room.idle_since_ms.load(Ordering::Relaxed)),
            );
            let evict =
                state.connections == 0 && (idle_for >= config.room_ttl || !state.initialized);
            let project_due = state.unprojected_since.is_some_and(|since| {
                now.duration_since(state.last_change) >= config.project_idle
                    || now.duration_since(since) >= config.project_max_delay
            });
            let flush_due = state
                .pending_since
                .is_some_and(|since| now.duration_since(since) >= config.flush_after);
            if evict {
                if self.persist_logged(&room, &mut state, true, true).await || state.closed {
                    state.closed = true;
                    self.forget(&room);
                }
            } else if project_due || flush_due {
                let compact = self.compaction_due(&state);
                self.persist_logged(&room, &mut state, project_due, compact)
                    .await;
            }
        }
        self.enforce_budget(false).await;
    }

    /// Evicts idle rooms (no connections), least recently used first, while the loaded documents
    /// exceed the memory budget. Returns whether the budget holds afterwards (with `for_new`,
    /// with room for one more document).
    async fn enforce_budget(&self, for_new: bool) -> bool {
        let budget = self.inner.config.memory_budget_bytes;
        let fits = |used: usize| {
            if for_new {
                used < budget
            } else {
                used <= budget
            }
        };
        if fits(self.memory_in_use()) {
            return true;
        }
        let mut idle: Vec<Arc<Room>> = self
            .rooms()
            .values()
            .filter(|room| room.connections.load(Ordering::Relaxed) == 0)
            .cloned()
            .collect();
        idle.sort_by_key(|room| room.idle_since_ms.load(Ordering::Relaxed));
        for room in idle {
            if fits(self.memory_in_use()) {
                return true;
            }
            let Ok(mut state) = Arc::clone(&room.state).try_lock_owned() else {
                continue;
            };
            if state.connections > 0 {
                continue;
            }
            if self.persist_logged(&room, &mut state, true, true).await || state.closed {
                state.closed = true;
                self.forget(&room);
            }
        }
        fits(self.memory_in_use())
    }

    /// Admits a connection of `peer` to the document's room. `Err` carries a close code.
    pub(crate) async fn join(
        &self,
        key: DocKey,
        peer: &Peer,
    ) -> Result<Joined, (u16, &'static str)> {
        let config = &self.inner.config;
        if self.inner.stopping.load(Ordering::SeqCst) {
            return Err((close::RESTART, "server restarting"));
        }
        if self.inner.connections.load(Ordering::SeqCst) >= config.max_connections {
            return Err((close::OVERLOADED, "too many connections"));
        }
        {
            let users = self
                .inner
                .users
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if users.get(&peer.user_id).copied().unwrap_or(0) >= config.max_connections_per_user {
                return Err((close::OVERLOADED, "too many connections for this user"));
            }
        }
        let live = self.rooms().contains_key(&key);
        if !live && !self.enforce_budget(true).await {
            return Err((close::OVERLOADED, "memory budget exceeded"));
        }
        let (room, mut state) = match self.room(key, true).await {
            Ok(room) => room,
            Err(CollabError::NotFound) => return Err((close::GONE, "document not found")),
            Err(CollabError::Overloaded) => return Err((close::OVERLOADED, "overloaded")),
            Err(error) => {
                tracing::warn!(doc = %key, error = %error, "co-editing room failed to load");
                return Err((close::OVERLOADED, "document unavailable"));
            }
        };
        if state.connections >= config.max_connections_per_page {
            return Err((close::OVERLOADED, "too many connections to this document"));
        }
        state.connections += 1;
        room.connections.store(state.connections, Ordering::Relaxed);
        self.inner.connections.fetch_add(1, Ordering::SeqCst);
        *self
            .inner
            .users
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(peer.user_id)
            .or_default() += 1;
        let connection = self.inner.next_connection.fetch_add(1, Ordering::Relaxed);
        let rx = room.tx.subscribe();
        let sv = state.awareness.doc().transact().state_vector();
        let mut greeting = vec![Message::Sync(SyncMessage::SyncStep1(sv)).encode_v1()];
        if let Ok(update) = state.awareness.update()
            && !update.clients.is_empty()
        {
            greeting.push(Message::Awareness(update).encode_v1());
        }
        let epoch = state.epoch.clone();
        drop(state);
        Ok(Joined {
            room,
            connection,
            rx,
            greeting,
            epoch,
        })
    }

    /// A connection left: its awareness states are removed (and everyone is told).
    pub(crate) async fn leave(&self, room: &Arc<Room>, connection: u64, peer: &Peer) {
        {
            let mut users = self
                .inner
                .users
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(count) = users.get_mut(&peer.user_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    users.remove(&peer.user_id);
                }
            }
        }
        self.inner.connections.fetch_sub(1, Ordering::SeqCst);
        let mut state = Arc::clone(&room.state).lock_owned().await;
        let owned: Vec<u64> = state
            .awareness_owners
            .iter()
            .filter(|(_, owner)| **owner == connection)
            .map(|(client, _)| *client)
            .collect();
        if !owned.is_empty() {
            let ids: Vec<ClientID> = owned.iter().map(|id| ClientID::new(*id)).collect();
            for (id, client) in ids.iter().zip(&owned) {
                state.awareness.remove_state(*id);
                state.awareness_owners.remove(client);
            }
            if let Ok(update) = state.awareness.update_with_clients(ids) {
                let frame = Message::Awareness(update).encode_v1();
                self.share(CollabMessage::Awareness {
                    key: room.key,
                    data: BASE64.encode(&frame),
                });
                let _ = room.tx.send(Outbound::Frame {
                    except: None,
                    data: Bytes::from(frame),
                });
            }
        }
        if state.color_holders.remove(&connection).is_some()
            && !state
                .color_holders
                .values()
                .any(|user| *user == peer.user_id)
        {
            state.user_colors.remove(&peer.user_id);
        }
        state.connections = state.connections.saturating_sub(1);
        room.connections.store(state.connections, Ordering::Relaxed);
        if state.connections == 0 {
            room.idle_since_ms.store(self.now_ms(), Ordering::Relaxed);
        }
    }

    /// Handles one inbound y-sync frame of a connection.
    pub(crate) async fn handle(
        &self,
        room: &Arc<Room>,
        connection: u64,
        peer: &Peer,
        data: &[u8],
        owned_clients: &mut HashSet<u64>,
        budget: &mut impl FnMut(MessageKind) -> bool,
    ) -> Handled {
        let mut handled = Handled {
            replies: Vec::new(),
            close: None,
        };
        let mut state = Arc::clone(&room.state).lock_owned().await;
        if state.closed || !state.initialized {
            handled.close = Some((close::RESTART, "document reloaded"));
            return handled;
        }
        let mut decoder = DecoderV1::new(Cursor::new(data));
        let reader = MessageReader::new(&mut decoder);
        let mut changed = false;
        for message in reader {
            let Ok(message) = message else {
                handled.close = Some((close::BAD_MESSAGE, "malformed message"));
                break;
            };
            let kind = match &message {
                Message::Awareness(_) | Message::AwarenessQuery => MessageKind::Awareness,
                _ => MessageKind::Sync,
            };
            if !budget(kind) {
                handled.close = Some((close::RATE_LIMITED, "too many messages"));
                break;
            }
            match message {
                Message::Sync(SyncMessage::SyncStep1(sv)) => {
                    let update = state
                        .awareness
                        .doc()
                        .transact()
                        .encode_state_as_update_v1(&sv);
                    handled
                        .replies
                        .push(Message::Sync(SyncMessage::SyncStep2(update)).encode_v1());
                }
                Message::Sync(SyncMessage::SyncStep2(_) | SyncMessage::Update(_))
                    if state.locked =>
                {
                    // A locked page takes no content changes. They are dropped, not applied; the
                    // lock and the unlock both close every socket with 4423, so clients start over
                    // from the server's document and refused edits never come back.
                }
                Message::Sync(SyncMessage::SyncStep2(update) | SyncMessage::Update(update)) => {
                    if state.state_bytes + update.len() > self.inner.config.max_doc_bytes {
                        handled.close = Some((close::TOO_LARGE, "document too large"));
                        break;
                    }
                    let applied = {
                        let doc = state.awareness.doc();
                        let mut txn = doc.transact_mut();
                        apply(&mut txn, &update)
                    };
                    if applied.is_err() {
                        state.take_captured();
                        handled.close = Some((close::BAD_MESSAGE, "malformed update"));
                        break;
                    }
                    let mut deltas = state.take_captured();
                    if !deltas.is_empty() {
                        changed = true;
                        for delta in &deltas {
                            broadcast_update(room, Some(connection), delta);
                        }
                        if let Ok(merged) = yrs::merge_updates_v1(&deltas) {
                            self.share(CollabMessage::Update {
                                key: room.key,
                                epoch: state.epoch.clone(),
                                data: BASE64.encode(merged),
                                user_id: peer.user_id,
                            });
                        }
                        // Links and file URLs can only arrive in updates that name them.
                        if (contains(&update, b"link") || contains(&update, b"url"))
                            && repair(&state, &room.policy())
                        {
                            for delta in state.take_captured() {
                                broadcast_update(room, None, &delta);
                                deltas.push(delta);
                            }
                        }
                        for delta in deltas {
                            state.state_bytes += delta.len();
                            state.pending.push(delta);
                        }
                        state.pending_updates += 1;
                        state.pending_since.get_or_insert_with(Instant::now);
                        state.mark_changed(Some(peer.user_id));
                    }
                }
                Message::Awareness(update) => {
                    if let Some(accepted) =
                        accept_awareness(&mut state, connection, peer, update, owned_clients)
                    {
                        let frame = Message::Awareness(accepted.clone()).encode_v1();
                        if state.awareness.apply_update(accepted).is_ok() {
                            self.share(CollabMessage::Awareness {
                                key: room.key,
                                data: BASE64.encode(&frame),
                            });
                            // Echo to everyone incl. the sender: y-websocket closes a socket
                            // that receives nothing for 30 s.
                            let _ = room.tx.send(Outbound::Frame {
                                except: None,
                                data: Bytes::from(frame),
                            });
                        }
                    }
                }
                Message::AwarenessQuery => {
                    if let Ok(update) = state.awareness.update() {
                        handled.replies.push(Message::Awareness(update).encode_v1());
                    }
                }
                Message::Auth(_) | Message::Custom(..) => {}
            }
        }
        if changed {
            self.set_bytes(room, &state);
            if state.pending_updates >= self.inner.config.flush_max_updates {
                let compact = self.compaction_due(&state);
                self.persist_logged(room, &mut state, false, compact).await;
            }
        }
        handled
    }

    /// Locks the document for a server-side content replacement. Pending edits are written and
    /// projected first, so the caller's transaction reads the current stored content.
    pub async fn write(&self, key: DocKey) -> Result<CollabWrite, CollabError> {
        let live = self.rooms().contains_key(&key);
        let (room, mut state) = self.room(key, false).await?;
        // A write that loaded a stored document (bulk writes load one for each item) respects the memory
        // budget like a new connection: idle rooms go first, and if that is not enough the write is refused.
        if !live && state.initialized && !self.enforce_budget(false).await {
            if state.connections == 0 {
                state.closed = true;
                self.forget(&room);
            }
            return Err(CollabError::Overloaded);
        }
        if state.initialized && (state.unprojected_since.is_some() || !state.pending.is_empty()) {
            self.persist(&room, &mut state, true, false).await?;
        }
        Ok(CollabWrite {
            hub: self.clone(),
            room,
            state: Some(state),
            staged: None,
            touched: false,
        })
    }

    /// Holds the page's room for a lock change (taken before the caller's transaction, like
    /// [`Self::write`]), so no client update slips in between the commit and the flag.
    pub async fn lock_change(&self, key: DocKey) -> Result<CollabLock, CollabError> {
        let (room, state) = self.room(key, false).await?;
        Ok(CollabLock {
            hub: self.clone(),
            room,
            state: Some(state),
        })
    }

    /// [`Self::flush`] of a page.
    pub async fn flush_page(&self, page_id: Id) {
        self.flush(DocKey::page(page_id)).await;
    }

    /// Writes pending edits of the document (if it is loaded) and projects them into its stored
    /// content now (before duplicate, history, reads). With other servers, it also projects the
    /// edits they logged and did not project yet.
    pub async fn flush(&self, key: DocKey) {
        let room = self.rooms().get(&key).cloned();
        if let Some(room) = room {
            let mut state = Arc::clone(&room.state).lock_owned().await;
            if !state.closed {
                self.persist_logged(&room, &mut state, true, false).await;
            }
            return;
        }
        if !self.shared() {
            return;
        }
        let sql = key.kind.sql(
            "SELECT EXISTS (SELECT 1 FROM page_collab_docs JOIN page_collab_updates \
             ON page_collab_updates.page_id = page_collab_docs.page_id \
             AND page_collab_updates.seq > page_collab_docs.projected_seq \
             WHERE page_collab_docs.page_id = $1)",
        );
        let behind = sqlx::query_scalar::<_, Flag>(sql)
            .bind(key.id.to_string())
            .fetch_one(&self.inner.pool)
            .await;
        if matches!(behind, Ok(Flag(true))) {
            // Loading reads the log and marks what was not projected.
            match self.room(key, false).await {
                Ok((room, mut state)) => {
                    self.persist_logged(&room, &mut state, true, false).await;
                }
                Err(error) => {
                    tracing::warn!(doc = %key, error = %error, "co-editing flush failed");
                }
            }
        }
    }

    /// [`Self::flush`] for every loaded document of the workspace, and with other servers for
    /// every document of it with edits that were not projected.
    pub async fn flush_workspace(&self, workspace_id: Id) {
        let mut docs: Vec<DocKey> = self
            .rooms()
            .values()
            .filter(|room| room.workspace_id.get() == Some(&workspace_id))
            .map(|room| room.key)
            .collect();
        if self.shared() {
            for kind in [DocKind::Page, DocKind::Task] {
                let sql = kind.sql(
                    "SELECT DISTINCT page_collab_docs.page_id FROM page_collab_docs \
                     JOIN page_collab_updates ON page_collab_updates.page_id = page_collab_docs.page_id \
                     AND page_collab_updates.seq > page_collab_docs.projected_seq \
                     WHERE page_collab_docs.workspace_id = $1",
                );
                let behind = sqlx::query_scalar::<_, String>(sql)
                    .bind(workspace_id.to_string())
                    .fetch_all(&self.inner.pool)
                    .await
                    .unwrap_or_default();
                docs.extend(
                    behind
                        .iter()
                        .filter_map(|id| id.parse::<Id>().ok())
                        .map(|id| DocKey { kind, id }),
                );
            }
            docs.sort_unstable_by_key(|key| (key.kind == DocKind::Task, key.id));
            docs.dedup();
        }
        for key in docs {
            self.flush(key).await;
        }
    }

    /// Makes every connection of the workspace (all workspaces with `None`) re-check its access
    /// now; connections that lost it are closed (4401/4403/4404).
    pub fn revalidate(&self, workspace_id: Option<Id>) {
        self.revalidate_here(workspace_id);
        self.tell(CollabMessage::Revalidate { workspace_id });
    }

    fn revalidate_here(&self, workspace_id: Option<Id>) {
        for room in self.rooms().values() {
            if workspace_id.is_none() || room.workspace_id.get() == workspace_id.as_ref() {
                let _ = room.tx.send(Outbound::Revalidate);
            }
        }
    }

    /// [`Self::revalidate`] on the database's hub, if it has one.
    pub fn revalidate_database(database: &Database, workspace_id: Option<Id>) {
        if let Some(hub) = Self::existing(database) {
            hub.revalidate(workspace_id);
        }
    }

    /// Server shutdown: writes and projects every document, closes every connection (1012).
    pub async fn shutdown(&self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        let rooms: Vec<Arc<Room>> = self.rooms().values().cloned().collect();
        for room in rooms {
            let mut state = Arc::clone(&room.state).lock_owned().await;
            if !state.closed {
                self.persist_logged(&room, &mut state, true, true).await;
                state.closed = true;
            }
            let _ = room.tx.send(Outbound::Close {
                code: close::RESTART,
                reason: "server restarting",
            });
            self.forget(&room);
        }
    }
}

/// Which token bucket a message draws from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MessageKind {
    Sync,
    Awareness,
}

/// A server-side replacement of a page's document, holding the room lock until it is committed
/// or dropped (see [`CollabHub::write`]). Dropping it after [`Self::replace`] without
/// [`Self::commit`] (the transaction failed) discards the room: its clients reload from storage.
pub struct CollabWrite {
    hub: CollabHub,
    room: Arc<Room>,
    state: Option<OwnedMutexGuard<RoomState>>,
    staged: Option<Staged>,
    touched: bool,
}

struct Staged {
    deltas: Vec<Vec<u8>>,
    seq: i64,
    bytes: usize,
}

impl CollabWrite {
    /// Whether the page has a collaborative document (otherwise only `content_json` changes and
    /// the document is built from it when the page is first opened).
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.state.as_ref().is_some_and(|state| state.initialized)
    }

    /// Replaces the document with `blocks` (sanitized) as one transaction and appends it to the
    /// update log inside `tx`; `content_json` is the caller's to write in the same transaction.
    pub async fn replace(
        &mut self,
        tx: &mut Transaction<'_, Any>,
        blocks: &[Value],
        actor_id: Id,
        now: TimestampMillis,
    ) -> Result<(), CollabError> {
        let Some(state) = self.state.as_mut() else {
            return Ok(());
        };
        if !state.initialized {
            return Ok(());
        }
        // The caller's write transaction: the replacement goes after every row it can see.
        if let Err(error) = self.hub.catch_up(tx, &self.room, state).await {
            if matches!(error, CollabError::Reset) {
                self.hub.close_room(&self.room, state, close::RESET);
            }
            return Err(error);
        }
        let sanitized = sanitize_blocks_for(blocks, &self.room.policy());
        self.touched = true;
        {
            let doc = state.awareness.doc();
            let mut txn = doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
            blocknote::replace_blocks(&mut txn, &fragment, &sanitized);
        }
        let deltas = state.take_captured();
        if deltas.is_empty() {
            return Ok(());
        }
        let merged = yrs::merge_updates_v1(&deltas)
            .map_err(|error| CollabError::Corrupt(error.to_string()))?;
        let workspace_id = *self.room.workspace_id.get().ok_or(CollabError::NotFound)?;
        let seq = append(
            tx,
            self.room.key,
            workspace_id,
            &merged,
            Some(actor_id),
            now,
        )
        .await?;
        sqlx::query(self.room.key.kind.sql(
            "UPDATE page_collab_docs SET projected_seq = $1, updated_at = $2 WHERE page_id = $3",
        ))
        .bind(seq)
        .bind(now.as_millis())
        .bind(self.room.key.id.to_string())
        .execute(&mut **tx)
        .await?;
        self.staged = Some(Staged {
            bytes: merged.len(),
            deltas,
            seq,
        });
        Ok(())
    }

    /// The transaction committed: broadcast the replacement to every connection.
    pub fn commit(mut self) {
        let Some(mut state) = self.state.take() else {
            return;
        };
        if let Some(staged) = self.staged.take() {
            for delta in &staged.deltas {
                state.state_bytes += delta.len();
                broadcast_update(&self.room, None, delta);
            }
            state.last_seq = staged.seq;
            state.projected_seq = staged.seq;
            state.log_updates += 1;
            state.log_bytes += staged.bytes;
            self.hub.set_bytes(&self.room, &state);
            self.hub.tell(CollabMessage::Logged { key: self.room.key });
        }
        if state.connections == 0 {
            // Written through: nothing to keep in memory for a page nobody has open.
            self.room.idle_since_ms.store(0, Ordering::Relaxed);
        }
        self.touched = false;
    }
}

impl Drop for CollabWrite {
    fn drop(&mut self) {
        let Some(mut state) = self.state.take() else {
            return;
        };
        if self.touched {
            // The in-memory document is ahead of storage: discard it; clients reconnect and
            // resync against the stored state.
            state.closed = true;
            let _ = self.room.tx.send(Outbound::Close {
                code: close::OVERLOADED,
                reason: "document reloaded",
            });
            self.hub.forget(&self.room);
        } else if state.connections == 0 {
            self.room.idle_since_ms.store(0, Ordering::Relaxed);
        }
    }
}

/// A page lock change in progress, holding the room (see [`CollabHub::lock_change`]). Dropping it
/// without [`Self::commit`] changes nothing.
pub struct CollabLock {
    hub: CollabHub,
    room: Arc<Room>,
    state: Option<OwnedMutexGuard<RoomState>>,
}

impl CollabLock {
    /// The lock change committed: from now on the room ignores (`locked`) or accepts content
    /// updates, and every connection is closed with 4423 so its client reloads the page (lock
    /// state) and resyncs a fresh document.
    pub fn commit(mut self, locked: bool) {
        let Some(mut state) = self.state.take() else {
            return;
        };
        if state.locked != locked {
            state.locked = locked;
            let _ = self.room.tx.send(Outbound::Close {
                code: close::LOCK_CHANGED,
                reason: if locked {
                    "page locked"
                } else {
                    "page unlocked"
                },
            });
        }
        // The other servers' rooms reload the lock with the document.
        self.hub.tell(CollabMessage::Close {
            key: self.room.key,
            code: close::LOCK_CHANGED,
        });
        if state.connections == 0 {
            self.room.idle_since_ms.store(0, Ordering::Relaxed);
        }
    }
}

impl Drop for CollabLock {
    fn drop(&mut self) {
        if let Some(state) = self.state.take()
            && state.connections == 0
        {
            self.room.idle_since_ms.store(0, Ordering::Relaxed);
        }
    }
}

fn apply(txn: &mut yrs::TransactionMut, data: &[u8]) -> Result<(), CollabError> {
    let update =
        Update::decode_v1(data).map_err(|error| CollabError::Corrupt(error.to_string()))?;
    txn.apply_update(update)
        .map_err(|error| CollabError::Corrupt(error.to_string()))
}

fn write_document(state: &RoomState, blocks: &[Value], policy: &LinkPolicy) {
    let sanitized = sanitize_blocks_for(blocks, policy);
    let doc = state.awareness.doc();
    let mut txn = doc.transact_mut();
    let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
    blocknote::write_blocks(&mut txn, &fragment, &sanitized);
}

/// Runs the link / file URL repair on the document; the deltas land in `captured`.
fn repair(state: &RoomState, policy: &LinkPolicy) -> bool {
    let doc = state.awareness.doc();
    let mut txn = doc.transact_mut();
    let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
    repair_document_for(&mut txn, &fragment, policy)
}

fn broadcast_update(room: &Room, except: Option<u64>, delta: &[u8]) {
    let frame = Message::Sync(SyncMessage::Update(delta.to_vec())).encode_v1();
    let _ = room.tx.send(Outbound::Frame {
        except,
        data: Bytes::from(frame),
    });
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Filters an awareness update to the client ids this connection may speak for and rewrites
/// each state's `user` to the session's user (names cannot be spoofed).
fn accept_awareness(
    state: &mut RoomState,
    connection: u64,
    peer: &Peer,
    update: AwarenessUpdate,
    owned_clients: &mut HashSet<u64>,
) -> Option<AwarenessUpdate> {
    let mut accepted = AwarenessUpdate {
        clients: HashMap::new(),
    };
    for (client, entry) in update.clients {
        let id = client.get();
        if state
            .awareness_owners
            .get(&id)
            .is_some_and(|owner| *owner != connection)
        {
            continue;
        }
        if entry.json.as_ref() == "null" {
            if owned_clients.remove(&id) {
                state.awareness_owners.remove(&id);
            }
            accepted.clients.insert(client, entry);
            continue;
        }
        if entry.json.len() > MAX_AWARENESS_STATE_BYTES
            || (!owned_clients.contains(&id)
                && owned_clients.len() >= MAX_AWARENESS_CLIENTS_PER_CONNECTION)
        {
            continue;
        }
        let mut value: Value = serde_json::from_str(&entry.json).unwrap_or(Value::Null);
        if !value.is_object() {
            value = Value::Object(serde_json::Map::new());
        }
        value["user"] = serde_json::json!({
            "id": peer.user_id.to_string(),
            "name": peer.name,
            "color": room_color(state, connection, peer.user_id),
        });
        owned_clients.insert(id);
        state.awareness_owners.insert(id, connection);
        accepted.clients.insert(
            client,
            yrs::sync::awareness::AwarenessUpdateEntry {
                clock: entry.clock,
                json: Arc::from(value.to_string()),
            },
        );
    }
    (!accepted.clients.is_empty()).then_some(accepted)
}

/// The user's cursor color on this page: their usual color (picked by id, so it stays the same
/// across visits) unless someone else in the room has it, then the next free one. Several tabs of
/// one user share a color; it is freed when their last connection leaves.
fn room_color(state: &mut RoomState, connection: u64, user_id: Id) -> &'static str {
    state.color_holders.insert(connection, user_id);
    let slot = if let Some(slot) = state.user_colors.get(&user_id) {
        *slot
    } else {
        let start = color_slot(user_id);
        let slot = (0..USER_COLORS.len())
            .map(|step| (start + step) % USER_COLORS.len())
            .find(|slot| !state.user_colors.values().any(|taken| taken == slot))
            .unwrap_or(start);
        state.user_colors.insert(user_id, slot);
        slot
    };
    USER_COLORS[slot]
}

/// The user's usual color slot: `hash * 31 + byte` over the id's text.
fn color_slot(user_id: Id) -> usize {
    let hash = user_id.to_string().bytes().fold(0_u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(u32::from(byte))
    });
    hash as usize % USER_COLORS.len()
}

fn random_epoch() -> String {
    Id::new_v7().to_string().replace('-', "")
}

/// The epoch of pages without a collaborative document (rotated by backup restores).
pub(crate) async fn generation(pool: &AnyPool) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT generation FROM page_collab_meta WHERE id = 1")
        .fetch_one(pool)
        .await
}

async fn append(
    tx: &mut Transaction<'_, Any>,
    key: DocKey,
    workspace_id: Id,
    data: &[u8],
    user_id: Option<Id>,
    now: TimestampMillis,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(key.kind.sql(
        "INSERT INTO page_collab_updates (page_id, workspace_id, data, user_id, created_at) \
         VALUES ($1, $2, $3, $4, $5) RETURNING seq",
    ))
    .bind(key.id.to_string())
    .bind(workspace_id.to_string())
    .bind(data)
    .bind(user_id.map(|id| id.to_string()))
    .bind(now.as_millis())
    .fetch_one(&mut **tx)
    .await
}

async fn compact_log(
    tx: &mut Transaction<'_, Any>,
    key: DocKey,
    snapshot: &[u8],
    seq: i64,
    now: TimestampMillis,
) -> Result<(), sqlx::Error> {
    sqlx::query(key.kind.sql(
        "UPDATE page_collab_docs SET snapshot = $1, snapshot_seq = $2, updated_at = $3 \
         WHERE page_id = $4",
    ))
    .bind(snapshot)
    .bind(seq)
    .bind(now.as_millis())
    .bind(key.id.to_string())
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        key.kind
            .sql("DELETE FROM page_collab_updates WHERE page_id = $1 AND seq <= $2"),
    )
    .bind(key.id.to_string())
    .bind(seq)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_on_a_page_get_different_colors_until_the_palette_runs_out() {
        let first = Id::new_v7();
        // A second user whose usual color is the same as the first one's.
        let clash = std::iter::repeat_with(Id::new_v7)
            .find(|id| color_slot(*id) == color_slot(first))
            .unwrap();
        let mut state = RoomState::new();
        let a = room_color(&mut state, 1, first);
        assert_eq!(
            a,
            USER_COLORS[color_slot(first)],
            "the usual color while it is free"
        );
        assert_eq!(
            room_color(&mut state, 2, first),
            a,
            "tabs of one user share a color"
        );
        let b = room_color(&mut state, 3, clash);
        assert_ne!(a, b);
        let others: Vec<&str> = (0..USER_COLORS.len() - 2)
            .map(|index| room_color(&mut state, 10 + index as u64, Id::new_v7()))
            .collect();
        let mut all: Vec<&str> = [a, b].into_iter().chain(others).collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), USER_COLORS.len(), "every color is used once");
        // The palette is full: a tenth person shares a color instead of failing.
        assert!(USER_COLORS.contains(&room_color(&mut state, 99, Id::new_v7())));
    }
}
