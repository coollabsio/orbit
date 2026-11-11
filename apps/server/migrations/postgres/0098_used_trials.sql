-- Orbit Cloud: the trials of email addresses that an account no longer uses (it was deleted, or its email changed),
-- so a new account with the same mailbox continues that trial instead of starting a new one.
-- email_key is a SHA-256 of the normalized address (apps/server/src/repositories/billing.rs, trial_email_key), not
-- the address.
CREATE TABLE used_trials (
    email_key TEXT PRIMARY KEY,
    trial_started_at BIGINT NOT NULL
);
