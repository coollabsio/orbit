-- The OpenID Connect provider can be saved in Admin → Sign-in providers, not only set with `ORBIT_OIDC_*`. Its row
-- also has the issuer URL and the name on the sign-in button.
ALTER TABLE sign_in_providers DROP CONSTRAINT sign_in_providers_provider_check;
ALTER TABLE sign_in_providers ADD CONSTRAINT sign_in_providers_provider_check
    CHECK (provider IN ('google', 'github', 'oidc'));
ALTER TABLE sign_in_providers ADD COLUMN issuer TEXT CHECK (length(issuer) BETWEEN 1 AND 255);
ALTER TABLE sign_in_providers ADD COLUMN display_name TEXT CHECK (length(display_name) BETWEEN 1 AND 200);
ALTER TABLE sign_in_providers ADD CONSTRAINT sign_in_providers_oidc_check
    CHECK ((provider = 'oidc') = (issuer IS NOT NULL AND display_name IS NOT NULL));
