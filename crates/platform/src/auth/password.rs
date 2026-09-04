use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::rngs::OsRng;
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

const COMMON_PASSWORDS: &[&str] = &[
    "123456789012",
    "letmeinletmein",
    "password1234",
    "qwertyqwerty",
];

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
            .iter()
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
