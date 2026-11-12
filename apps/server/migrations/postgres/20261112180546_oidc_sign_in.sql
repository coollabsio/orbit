-- Postgres: the same change as apps/server/migrations/20261112180546_oidc_sign_in.sql
-- Sign in with an OpenID Connect provider (`ORBIT_OIDC_*`). Its links are `provider = 'oidc'` with the subject
-- "{issuer} {sub}", so the subject may be longer. sign_in_providers keeps its CHECK: only the environment sets the
-- OpenID Connect client.
ALTER TABLE user_identities DROP CONSTRAINT user_identities_provider_check;
ALTER TABLE user_identities ADD CONSTRAINT user_identities_provider_check
    CHECK (provider IN ('google', 'github', 'oidc'));
ALTER TABLE user_identities DROP CONSTRAINT user_identities_subject_check;
ALTER TABLE user_identities ADD CONSTRAINT user_identities_subject_check
    CHECK (length(subject) BETWEEN 1 AND 512);
