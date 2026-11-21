-- A first OpenID Connect sign-in creates an account, also when registration is closed (Admin → Sign-in providers).
ALTER TABLE instance_settings ADD COLUMN oidc_provision BIGINT NOT NULL DEFAULT 0;
