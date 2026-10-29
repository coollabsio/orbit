-- Sign in with Google and GitHub (apps/server/src/sign_in_providers.rs).
--
-- The OAuth app an installation admin saved for each provider. A row means the provider is offered on the
-- sign-in pages. The secret is ChaCha20-Poly1305 under the server app key (secret_box.rs), never plain text.
CREATE TABLE sign_in_providers (
    provider TEXT PRIMARY KEY CHECK (provider IN ('google', 'github')),
    client_id TEXT NOT NULL CHECK (length(client_id) BETWEEN 1 AND 256),
    client_secret BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- A provider account that signs in to an Orbit account. `subject` is the provider's stable user id (Google `sub`,
-- GitHub numeric `id`), never the email: emails change. `email` is what the provider reported, for display only.
-- Links stay when an admin removes the provider, so saving it again restores them.
CREATE TABLE user_identities (
    provider TEXT NOT NULL CHECK (provider IN ('google', 'github')),
    subject TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 256),
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    email TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (provider, subject),
    UNIQUE (user_id, provider)
);

-- When the session last proved who uses it with a provider "confirm" flow. NULL: only its sign-in (created_at).
-- Accounts without a password confirm security changes with a recent sign-in instead of the password.
ALTER TABLE sessions ADD COLUMN reauthenticated_at INTEGER;
