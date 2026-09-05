use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use rand::rngs::OsRng;
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

use super::token::hash_token;
use crate::{Id, TimestampMillis};

const DAY_MILLIS: i64 = 24 * 60 * 60 * 1_000;
const IDLE_LIFETIME: i64 = 30 * DAY_MILLIS;
const ABSOLUTE_LIFETIME: i64 = 90 * DAY_MILLIS;
const ACTIVITY_WRITE_INTERVAL: i64 = 5 * 60 * 1_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct AuthenticatedUser {
    #[schema(value_type = String)]
    pub id: Id,
    pub email: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
pub struct SessionRecord {
    #[schema(value_type = String)]
    pub id: Id,
    pub user: AuthenticatedUser,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub last_activity_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub idle_expires_at: TimestampMillis,
    #[schema(value_type = String, format = DateTime)]
    pub absolute_expires_at: TimestampMillis,
    pub current: bool,
}

#[derive(Clone, Eq, PartialEq)]
pub struct IssuedSession {
    pub id: Id,
    pub token: String,
    pub idle_expires_at: TimestampMillis,
    pub absolute_expires_at: TimestampMillis,
}

impl fmt::Debug for IssuedSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedSession")
            .field("id", &self.id)
            .field("token", &"[REDACTED]")
            .field("idle_expires_at", &self.idle_expires_at)
            .field("absolute_expires_at", &self.absolute_expires_at)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum SessionError {
    #[error("session is invalid or expired")]
    Invalid,
}

pub trait SessionStore: Send + Sync {
    fn create(
        &self,
        user: AuthenticatedUser,
        now: TimestampMillis,
    ) -> Result<IssuedSession, SessionError>;
    fn authenticate(
        &self,
        token: &str,
        now: TimestampMillis,
    ) -> Result<AuthenticatedUser, SessionError>;
    fn revoke(&self, session_id: Id) -> Result<(), SessionError>;
    fn revoke_token(&self, token: &str) -> Result<(), SessionError>;
    fn revoke_user(&self, user_id: Id);
    fn list(&self, user_id: Id, now: TimestampMillis) -> Vec<SessionRecord>;
}

#[derive(Clone, Debug)]
struct StoredSession {
    record: SessionRecord,
    token_hash: [u8; 32],
    activity_write_count: u64,
}

#[derive(Debug, Default)]
pub struct InMemorySessionStore {
    sessions: Mutex<HashMap<Id, StoredSession>>,
}

impl InMemorySessionStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn get(&self, session_id: Id) -> Option<SessionRecord> {
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .get(&session_id)
            .map(|session| session.record.clone())
    }

    #[must_use]
    pub fn activity_write_count(&self, session_id: Id) -> Option<u64> {
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .get(&session_id)
            .map(|session| session.activity_write_count)
    }
}

impl SessionStore for InMemorySessionStore {
    fn create(
        &self,
        user: AuthenticatedUser,
        now: TimestampMillis,
    ) -> Result<IssuedSession, SessionError> {
        let id = Id::new_v7();
        let mut random = [0_u8; 32];
        OsRng.fill_bytes(&mut random);
        let token = URL_SAFE_NO_PAD.encode(random);
        let idle_expires_at = add_millis(now, IDLE_LIFETIME);
        let absolute_expires_at = add_millis(now, ABSOLUTE_LIFETIME);
        let record = SessionRecord {
            id,
            user,
            created_at: now,
            last_activity_at: now,
            idle_expires_at,
            absolute_expires_at,
            current: false,
        };
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .insert(
                id,
                StoredSession {
                    record,
                    token_hash: hash_token(&token),
                    activity_write_count: 0,
                },
            );
        Ok(IssuedSession {
            id,
            token,
            idle_expires_at,
            absolute_expires_at,
        })
    }

    fn authenticate(
        &self,
        token: &str,
        now: TimestampMillis,
    ) -> Result<AuthenticatedUser, SessionError> {
        let wanted = hash_token(token);
        let mut sessions = self.sessions.lock().expect("session mutex poisoned");
        let id = sessions
            .iter()
            .find(|(_, stored)| constant_time_eq(&stored.token_hash, &wanted))
            .map(|(id, _)| *id)
            .ok_or(SessionError::Invalid)?;
        let session = sessions.get_mut(&id).ok_or(SessionError::Invalid)?;
        if now >= session.record.idle_expires_at || now >= session.record.absolute_expires_at {
            sessions.remove(&id);
            return Err(SessionError::Invalid);
        }
        if now.as_millis() - session.record.last_activity_at.as_millis() >= ACTIVITY_WRITE_INTERVAL
        {
            session.record.last_activity_at = now;
            session.record.idle_expires_at = TimestampMillis::from_millis(
                add_millis(now, IDLE_LIFETIME)
                    .as_millis()
                    .min(session.record.absolute_expires_at.as_millis()),
            );
            session.activity_write_count += 1;
        }
        Ok(session.record.user.clone())
    }

    fn revoke(&self, session_id: Id) -> Result<(), SessionError> {
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .remove(&session_id)
            .map(|_| ())
            .ok_or(SessionError::Invalid)
    }

    fn revoke_token(&self, token: &str) -> Result<(), SessionError> {
        let wanted = hash_token(token);
        let mut sessions = self.sessions.lock().expect("session mutex poisoned");
        let id = sessions
            .iter()
            .find(|(_, stored)| constant_time_eq(&stored.token_hash, &wanted))
            .map(|(id, _)| *id)
            .ok_or(SessionError::Invalid)?;
        sessions.remove(&id);
        Ok(())
    }

    fn revoke_user(&self, user_id: Id) {
        self.sessions
            .lock()
            .expect("session mutex poisoned")
            .retain(|_, session| session.record.user.id != user_id);
    }

    fn list(&self, user_id: Id, now: TimestampMillis) -> Vec<SessionRecord> {
        let mut sessions = self.sessions.lock().expect("session mutex poisoned");
        sessions.retain(|_, session| {
            now < session.record.idle_expires_at && now < session.record.absolute_expires_at
        });
        sessions
            .values()
            .filter(|session| session.record.user.id == user_id)
            .map(|session| session.record.clone())
            .collect()
    }
}

fn add_millis(now: TimestampMillis, duration: i64) -> TimestampMillis {
    TimestampMillis::from_millis(now.as_millis().saturating_add(duration))
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let mut difference = 0_u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}
