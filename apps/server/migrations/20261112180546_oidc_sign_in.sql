-- SQLite
-- Sign in with an OpenID Connect provider (`ORBIT_OIDC_*`). Its links are `provider = 'oidc'` with the subject
-- "{issuer} {sub}", so the subject may be longer. The client is not saved: only the environment sets it, so
-- sign_in_providers keeps its CHECK. SQLite cannot change a CHECK: rebuild the table (no index, trigger or foreign
-- key names it).
CREATE TABLE user_identities_old AS SELECT * FROM user_identities;
DROP TABLE user_identities;
CREATE TABLE user_identities (
    provider TEXT NOT NULL CHECK (provider IN ('google', 'github', 'oidc')),
    subject TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 512),
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    email TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (provider, subject),
    UNIQUE (user_id, provider)
);
INSERT INTO user_identities (provider, subject, user_id, email, created_at)
    SELECT provider, subject, user_id, email, created_at FROM user_identities_old;
DROP TABLE user_identities_old;
