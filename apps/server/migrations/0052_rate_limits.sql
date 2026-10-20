-- Request counts of the sensitive endpoint classes (sign-in, recovery and registration,
-- invitations) for each minute, shared by every server of the database
-- (crates/platform/src/http/rate_limit.rs). `bucket` names the class and the client, credential or
-- workspace.
CREATE TABLE rate_limit_windows (
    bucket TEXT NOT NULL,
    window_minute INTEGER NOT NULL,
    hits INTEGER NOT NULL,
    PRIMARY KEY (bucket, window_minute)
);

-- Emails sent to an address anyone can type (registration and password reset links) and
-- invitation emails, for the per-address, per-workspace and per-inviter caps
-- (apps/server/src/email_quota.rs). One row per counted email; rows older than a day are deleted.
CREATE TABLE email_sends (
    scope TEXT NOT NULL,
    subject TEXT NOT NULL,
    sent_at INTEGER NOT NULL
);

CREATE INDEX email_sends_by_subject ON email_sends (scope, subject, sent_at);
