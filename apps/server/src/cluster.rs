//! Postgres only: the servers of one database tell each other about live sockets and co-editing
//! over `LISTEN`/`NOTIFY` on one channel. Every message is JSON with the sending server's id.
//!
//! - The live socket ([`crate::live`]): new events of a workspace (each server reads them from
//!   `live_events`), signals that are not kept, and what each server has of every member.
//! - Co-editing ([`crate::collab`]): small document and cursor updates as they happen, new rows of
//!   a page's update log, rooms to close (reset, lock change), access to check again.
//! - A beat every few seconds: a server that is not heard from for a while stopped, and its
//!   members go offline.
//!
//! A message is at most [`MAX_PAYLOAD`] bytes (Postgres refuses 8000 or more); a longer one is
//! not sent. Messages can be lost (a dropped listener connection): the receivers then read the
//! database again, and [`Cluster::send`] callers keep the database as the truth.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use orbit_platform::{Database, Id};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgListener, PgPool};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::collab::{CollabHub, CollabMessage};
use crate::live::{LiveHub, LiveMessage};

const CHANNEL: &str = "orbit_cluster";
/// Messages that may wait to be sent.
const QUEUED_MESSAGES: usize = 10_000;
const MAX_PAYLOAD: usize = 7_900;
/// How often a server beats, tells what changed with time and reads what it may have missed.
const TICK: Duration = Duration::from_secs(5);
/// A server not heard from for this long stopped.
pub(crate) const SERVER_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body", rename_all = "snake_case")]
pub(crate) enum Message {
    Beat,
    Live(LiveMessage),
    Collab(CollabMessage),
    /// The root user saved the storage settings (Admin → Storage).
    StorageChanged,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    server: String,
    #[serde(flatten)]
    message: Message,
}

/// This server among the servers of its database.
pub struct Cluster {
    server: String,
    /// While [`Member::run`] runs: messages for the other servers.
    outgoing: Mutex<Option<mpsc::Sender<Message>>>,
    /// The other servers, and when each was last heard from.
    heard: Mutex<HashMap<String, Instant>>,
}

impl Cluster {
    /// The cluster member of this database (one for each open database).
    #[must_use]
    pub fn of(database: &Database) -> Arc<Self> {
        database.extension(|| Self {
            server: Id::new_v7().to_string(),
            outgoing: Mutex::new(None),
            heard: Mutex::new(HashMap::new()),
        })
    }

    fn outgoing(&self) -> std::sync::MutexGuard<'_, Option<mpsc::Sender<Message>>> {
        self.outgoing.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many other servers of the database were heard from lately.
    #[must_use]
    pub fn others(&self) -> usize {
        self.heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter(|heard| heard.elapsed() <= SERVER_TIMEOUT)
            .count()
    }

    /// Whether other servers can hear this one ([`Member::run`] runs).
    pub(crate) fn running(&self) -> bool {
        self.outgoing().is_some()
    }

    /// Queues a message for the other servers, while [`Member::run`] runs. A full queue loses
    /// it.
    pub(crate) fn send(&self, message: Message) {
        if let Some(outgoing) = &*self.outgoing()
            && outgoing.try_send(message).is_err()
        {
            tracing::warn!("cluster queue full: a message for the other servers is lost");
        }
    }
}

/// What the listener task hands over.
enum Heard {
    Message(Envelope),
    /// The listener lost its connection: messages since then are lost.
    Lost,
}

/// This server's part of the cluster, subscribed and ready to run.
pub struct Member {
    database: Database,
    /// Postgres: the subscribed listener; SQLite has no other servers.
    listener: Option<(PgPool, PgListener)>,
}

/// Subscribes to the other servers' messages (Postgres). The server does this before it accepts
/// connections, so it misses nothing that its first clients do.
pub async fn join(database: &Database) -> Result<Member, sqlx::Error> {
    let listener = match database.postgres_pool() {
        Some(postgres) => Some((postgres.clone(), connect(postgres).await?)),
        None => None,
    };
    Ok(Member {
        database: database.clone(),
        listener,
    })
}

impl Member {
    /// Tells the other servers of the database about this server's live sockets and co-editing,
    /// and hears theirs, until `shutdown`. On SQLite (one server) it only waits.
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), String> {
        let Some((postgres, listener)) = self.listener else {
            shutdown.cancelled().await;
            return Ok(());
        };
        let database = self.database;
        let cluster = Cluster::of(&database);
        let (sender, outgoing) = mpsc::channel(QUEUED_MESSAGES);
        let (heard_sender, heard) = mpsc::channel(QUEUED_MESSAGES);
        let listener = tokio::spawn(listen(listener, heard_sender));
        *cluster.outgoing() = Some(sender);
        let result = serve(&database, &cluster, &postgres, outgoing, heard, shutdown).await;
        *cluster.outgoing() = None;
        listener.abort();
        result
    }
}

/// Receives notifications. It only ever waits in `try_recv`, which must not be cancelled while
/// the cluster runs; [`Member::run`] aborts it when it stops. The listener reconnects by itself.
async fn listen(mut listener: PgListener, heard: mpsc::Sender<Heard>) {
    loop {
        let item = match listener.try_recv().await {
            Ok(Some(notification)) => match serde_json::from_str(notification.payload()) {
                Ok(envelope) => Heard::Message(envelope),
                Err(error) => {
                    tracing::warn!(error = %error, "cluster message");
                    continue;
                }
            },
            Ok(None) => Heard::Lost,
            Err(error) => {
                tracing::warn!(error = %error, "cluster listener");
                tokio::time::sleep(Duration::from_secs(1)).await;
                Heard::Lost
            }
        };
        if heard.send(item).await.is_err() {
            return;
        }
    }
}

async fn connect(postgres: &PgPool) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect_with(postgres).await?;
    listener.listen(CHANNEL).await?;
    Ok(listener)
}

async fn serve(
    database: &Database,
    cluster: &Cluster,
    postgres: &PgPool,
    mut outgoing: mpsc::Receiver<Message>,
    mut heard: mpsc::Receiver<Heard>,
    shutdown: CancellationToken,
) -> Result<(), String> {
    let send = |message: Message| async move {
        let envelope = Envelope {
            server: cluster.server.clone(),
            message,
        };
        let payload = match serde_json::to_string(&envelope) {
            Ok(payload) if payload.len() <= MAX_PAYLOAD => payload,
            Ok(_) => {
                tracing::warn!("cluster message too long; not sent");
                return;
            }
            Err(error) => {
                tracing::warn!(error = %error, "cluster message");
                return;
            }
        };
        if let Err(error) = sqlx::query("SELECT pg_notify($1, $2)")
            .bind(CHANNEL)
            .bind(payload)
            .execute(postgres)
            .await
        {
            tracing::warn!(error = %error, "cluster send");
        }
    };
    let live = LiveHub::of(database);
    let mut warned_local_attachments = false;
    // This server tells what it has, and the others tell it what they have.
    live.joined_cluster();
    let mut tick = tokio::time::interval(TICK);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            message = outgoing.recv() => match message {
                Some(message) => send(message).await,
                None => return Ok(()),
            },
            item = heard.recv() => match item {
                Some(Heard::Message(envelope)) if envelope.server != cluster.server => {
                    cluster
                        .heard
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(envelope.server.clone(), Instant::now());
                    live.heard_server(&envelope.server);
                    match envelope.message {
                        Message::Beat => {}
                        Message::StorageChanged => reload_storage(database).await,
                        Message::Live(message) => live.receive(&envelope.server, message).await,
                        Message::Collab(message) => {
                            // Access changed: the live and event sockets check theirs too.
                            if let CollabMessage::Revalidate { workspace_id } = &message {
                                live.revalidate_here(*workspace_id);
                            }
                            if let Some(collab) = CollabHub::existing(database) {
                                collab.receive(message).await;
                            }
                        }
                    }
                }
                Some(Heard::Message(_)) => {}
                Some(Heard::Lost) => {
                    // Messages may be lost: read the database again, and every server tells
                    // what it has again.
                    live.catch_up().await;
                    live.joined_cluster();
                    reload_storage(database).await;
                    if let Some(collab) = CollabHub::existing(database) {
                        collab.catch_up_all().await;
                    }
                }
                None => return Err("cluster listener stopped".to_owned()),
            },
            _ = tick.tick() => {
                send(Message::Beat).await;
                live.tick(SERVER_TIMEOUT).await;
                reload_storage(database).await;
                if !warned_local_attachments
                    && cluster.others() > 0
                    && crate::object_storage::attachments_in_s3(database) == Some(false)
                {
                    // Each server has its own disk: files uploaded to one are missing on the others,
                    // and a backup holds one server's files only.
                    tracing::warn!(
                        "other servers use this database, but attachments are on this server's disk: \
                         turn on attachments in S3 (Admin → Storage)"
                    );
                    warned_local_attachments = true;
                }
                if let Some(collab) = CollabHub::existing(database) {
                    collab.catch_up_all().await;
                }
            }
        }
    }
}

async fn reload_storage(database: &Database) {
    if let Err(error) = crate::object_storage::reload_saved(database).await {
        tracing::warn!(%error, "storage settings saved by another server");
    }
}
