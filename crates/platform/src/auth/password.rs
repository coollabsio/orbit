use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::rngs::OsRng;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Semaphore;
use unicode_segmentation::UnicodeSegmentation;

pub const COMMON_PASSWORD_DATASET_VERSION: &str = "2026-09-04.v1";
const COMMON_PASSWORDS: &str = include_str!("common-passwords-2026-09-04.v1.txt");

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum PasswordError {
    #[error("password must contain between 12 and 128 characters")]
    InvalidLength,
    #[error("password is too common")]
    CommonPassword,
    #[error("stored password hash is invalid")]
    InvalidHash,
    #[error("password hashing failed")]
    HashingFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PasswordVerification {
    pub valid: bool,
    pub replacement_hash: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PasswordService {
    params: Params,
}

impl PasswordService {
    pub fn with_argon2_params(
        memory_cost_kib: u32,
        time_cost: u32,
        parallelism: u32,
    ) -> Result<Self, PasswordError> {
        let params = Params::new(memory_cost_kib, time_cost, parallelism, None)
            .map_err(|_| PasswordError::HashingFailed)?;
        Ok(Self { params })
    }

    pub fn hash(&self, password: &str) -> Result<String, PasswordError> {
        self.validate(password)?;
        let salt = SaltString::generate(&mut OsRng);
        self.argon2()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| PasswordError::HashingFailed)
    }

    pub fn validate(&self, password: &str) -> Result<(), PasswordError> {
        let length = password.graphemes(true).count();
        if !(12..=128).contains(&length) {
            return Err(PasswordError::InvalidLength);
        }
        if COMMON_PASSWORDS
            .lines()
            .any(|common| password.eq_ignore_ascii_case(common))
        {
            return Err(PasswordError::CommonPassword);
        }
        Ok(())
    }

    pub fn verify(
        &self,
        password: &str,
        encoded_hash: &str,
    ) -> Result<PasswordVerification, PasswordError> {
        let parsed = PasswordHash::new(encoded_hash).map_err(|_| PasswordError::InvalidHash)?;
        let valid = self
            .argon2()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok();
        if !valid {
            return Ok(PasswordVerification {
                valid: false,
                replacement_hash: None,
            });
        }

        let replacement_hash = if self.needs_rehash(&parsed) {
            Some(self.hash(password)?)
        } else {
            None
        };
        Ok(PasswordVerification {
            valid: true,
            replacement_hash,
        })
    }

    fn argon2(&self) -> Argon2<'_> {
        Argon2::new(Algorithm::Argon2id, Version::V0x13, self.params.clone())
    }

    fn needs_rehash(&self, hash: &PasswordHash<'_>) -> bool {
        hash.algorithm.as_str() != "argon2id"
            || hash.version != Some(Version::V0x13.into())
            || hash.params.get_decimal("m") != Some(self.params.m_cost())
            || hash.params.get_decimal("t") != Some(self.params.t_cost())
            || hash.params.get_decimal("p") != Some(self.params.p_cost())
    }
}

#[derive(Clone, Debug)]
pub struct PasswordExecutor {
    service: PasswordService,
    permits: Arc<Semaphore>,
    max_parallelism: usize,
}

impl PasswordExecutor {
    pub fn new(service: PasswordService, max_parallelism: usize) -> Result<Self, PasswordError> {
        if max_parallelism == 0 {
            return Err(PasswordError::HashingFailed);
        }
        Ok(Self {
            service,
            permits: Arc::new(Semaphore::new(max_parallelism)),
            max_parallelism,
        })
    }

    #[must_use]
    pub const fn max_parallelism(&self) -> usize {
        self.max_parallelism
    }

    pub async fn hash(&self, password: String) -> Result<String, PasswordError> {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| PasswordError::HashingFailed)?;
        let service = self.service.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            service.hash(&password)
        })
        .await
        .map_err(|_| PasswordError::HashingFailed)?
    }

    pub async fn verify(
        &self,
        password: String,
        encoded_hash: String,
    ) -> Result<PasswordVerification, PasswordError> {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| PasswordError::HashingFailed)?;
        let service = self.service.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            service.verify(&password, &encoded_hash)
        })
        .await
        .map_err(|_| PasswordError::HashingFailed)?
    }
}
