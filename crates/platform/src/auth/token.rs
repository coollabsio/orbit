use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::TimestampMillis;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TokenKind {
    Setup,
    Recovery,
}

#[derive(Clone, Eq, PartialEq)]
pub struct IssuedToken {
    pub token: String,
    pub expires_at: TimestampMillis,
}

impl fmt::Debug for IssuedToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedToken")
            .field("token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum TokenError {
    #[error("token is invalid or expired")]
    Invalid,
    #[error("setup has already been completed")]
    SetupComplete,
}

#[derive(Clone, Debug)]
struct TokenRecord {
    kind: TokenKind,
    subject: String,
    expires_at: TimestampMillis,
}

#[derive(Debug, Default)]
struct TokenState {
    records: HashMap<[u8; 32], TokenRecord>,
    setup_complete: bool,
}

#[derive(Debug, Default)]
pub struct OneTimeTokenStore {
    state: Mutex<TokenState>,
}

impl OneTimeTokenStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue(
        &self,
        kind: TokenKind,
        subject: impl Into<String>,
        now: TimestampMillis,
        lifetime: Duration,
    ) -> Result<IssuedToken, TokenError> {
        let mut state = self.state.lock().expect("token mutex poisoned");
        if kind == TokenKind::Setup && state.setup_complete {
            return Err(TokenError::SetupComplete);
        }
        let token = generate_opaque_token();
        let expires_at = TimestampMillis::from_millis(
            now.as_millis()
                .saturating_add(lifetime.as_millis().min(i64::MAX as u128) as i64),
        );
        state.records.insert(
            hash_token(&token),
            TokenRecord {
                kind,
                subject: subject.into(),
                expires_at,
            },
        );
        Ok(IssuedToken { token, expires_at })
    }

    pub fn consume(
        &self,
        kind: TokenKind,
        token: &str,
        now: TimestampMillis,
    ) -> Result<String, TokenError> {
        let wanted = hash_token(token);
        let mut state = self.state.lock().expect("token mutex poisoned");
        let matched = state
            .records
            .keys()
            .find(|stored| constant_time_eq(stored, &wanted))
            .copied()
            .ok_or(TokenError::Invalid)?;
        let record = state.records.get(&matched).ok_or(TokenError::Invalid)?;
        if record.kind != kind || now >= record.expires_at {
            return Err(TokenError::Invalid);
        }
        let record = state.records.remove(&matched).ok_or(TokenError::Invalid)?;
        if kind == TokenKind::Setup {
            if state.setup_complete {
                return Err(TokenError::SetupComplete);
            }
            state.setup_complete = true;
            state
                .records
                .retain(|_, candidate| candidate.kind != TokenKind::Setup);
        }
        Ok(record.subject)
    }

    #[must_use]
    pub fn persisted_hashes(&self) -> Vec<[u8; 32]> {
        self.state
            .lock()
            .expect("token mutex poisoned")
            .records
            .keys()
            .copied()
            .collect()
    }
}

#[must_use]
pub fn generate_opaque_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let mut difference = 0_u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}
