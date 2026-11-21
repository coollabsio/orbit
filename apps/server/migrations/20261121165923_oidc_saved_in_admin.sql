-- The OpenID Connect provider can be saved in Admin → Sign-in providers, not only set with `ORBIT_OIDC_*`. Its row
-- also has the issuer URL and the name on the sign-in button. SQLite cannot change a CHECK: rebuild the table (no
-- index, trigger or foreign key names it).
CREATE TABLE sign_in_providers_old AS SELECT * FROM sign_in_providers;
DROP TABLE sign_in_providers;
CREATE TABLE sign_in_providers (
    provider TEXT PRIMARY KEY CHECK (provider IN ('google', 'github', 'oidc')),
    client_id TEXT NOT NULL CHECK (length(client_id) BETWEEN 1 AND 256),
    client_secret BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    issuer TEXT CHECK (length(issuer) BETWEEN 1 AND 255),
    display_name TEXT CHECK (length(display_name) BETWEEN 1 AND 200),
    CHECK ((provider = 'oidc') = (issuer IS NOT NULL AND display_name IS NOT NULL))
);
INSERT INTO sign_in_providers (provider, client_id, client_secret, created_at, updated_at)
    SELECT provider, client_id, client_secret, created_at, updated_at FROM sign_in_providers_old;
DROP TABLE sign_in_providers_old;
