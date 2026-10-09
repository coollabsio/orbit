-- Two-factor sign-in and passkeys.
--
-- An authenticator app (TOTP). The secret is ChaCha20-Poly1305 under the server app key (secret_box.rs), never
-- plain text. The row exists from the moment the user starts the setup; sign-in asks for a code only once a first
-- code confirmed it (`enabled_at`). A code is good once: `last_used_step` is the newest 30-second step accepted.
CREATE TABLE user_totp (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    secret BLOB NOT NULL,
    enabled_at INTEGER,
    last_used_step INTEGER NOT NULL DEFAULT 0 CHECK (last_used_step >= 0),
    created_at INTEGER NOT NULL
);

-- One-time codes that replace an authenticator code when the device is lost. Only a SHA-256 of each code is kept;
-- a code is deleted when it is used.
CREATE TABLE user_recovery_codes (
    user_id TEXT NOT NULL REFERENCES users(id),
    code_hash BLOB NOT NULL CHECK (length(code_hash) = 32),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, code_hash)
);

-- Passkeys (WebAuthn credentials). A passkey signs in on its own: it is a device the user has, unlocked with a
-- fingerprint, face or PIN. `passkey` is the webauthn-rs credential as JSON (public key and signature counter).
CREATE TABLE user_passkeys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    -- base64url, as the browser sends it back when signing in.
    credential_id TEXT NOT NULL UNIQUE CHECK (length(credential_id) BETWEEN 1 AND 1400),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    passkey TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE INDEX user_passkeys_user ON user_passkeys (user_id);
