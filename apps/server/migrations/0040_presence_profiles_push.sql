-- Presence and custom status, profile details and private notes, and push notifications.
-- One migration for the three features: they were built together.

-- 1. Presence and custom status

-- The status a user sets for themselves: a presence (shown on their avatar) and an optional custom
-- status (an emoji and a short text) that can end at a set time. One row per user, the same in
-- every workspace, as the display name is. No row means "online" and no custom status.
-- `invisible` shows the user as offline to everybody else.
-- `expires_at` ends the custom status only; the presence stays until the user changes it.
-- Status changes are not audited: they are frequent and say nothing about the workspace's data.
-- Users are never hard-deleted, so the user reference needs no ON DELETE action.
CREATE TABLE user_status (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    presence TEXT NOT NULL CHECK (presence IN ('online', 'idle', 'dnd', 'invisible')),
    emoji TEXT CHECK (emoji IS NULL OR length(emoji) BETWEEN 1 AND 32),
    text TEXT CHECK (text IS NULL OR length(text) BETWEEN 1 AND 100),
    expires_at INTEGER,
    updated_at INTEGER NOT NULL,
    CHECK (expires_at IS NULL OR emoji IS NOT NULL OR text IS NOT NULL)
);

-- 2. Profile details and private notes

-- What a profile popover shows of a user, beside the name and picture: a job title, pronouns,
-- an IANA time zone (the web app shows the local time from it) and a short bio. All optional and,
-- like the display name, the same in every workspace. The server bounds the lengths; the web app
-- owns the list of time zones.
ALTER TABLE users ADD COLUMN title TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 80);
ALTER TABLE users ADD COLUMN pronouns TEXT CHECK (pronouns IS NULL OR length(pronouns) BETWEEN 1 AND 40);
ALTER TABLE users ADD COLUMN timezone TEXT CHECK (timezone IS NULL OR length(timezone) BETWEEN 1 AND 64);
ALTER TABLE users ADD COLUMN bio TEXT CHECK (bio IS NULL OR length(bio) BETWEEN 1 AND 500);

-- A phone number on the profile, for the people who share a workspace with the user. Optional
-- and, like the other profile parts, the same in every workspace. It is text as the user wrote
-- it (digits, spaces and `+ - ( ) .`); the server does not call or verify it.
ALTER TABLE users ADD COLUMN phone TEXT CHECK (phone IS NULL OR length(phone) BETWEEN 1 AND 32);

-- A private note one user keeps about another. Only its author can read it; it is not audited.
-- Users are never hard-deleted, so the user references need no ON DELETE action.
CREATE TABLE user_notes (
    author_id TEXT NOT NULL REFERENCES users(id),
    subject_id TEXT NOT NULL REFERENCES users(id),
    body TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 1000),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (author_id, subject_id)
) WITHOUT ROWID;

-- 3. Push notifications

-- The Web Push protocol.
--
-- A browser's subscription: where to send (`endpoint`, at the browser vendor's push service) and
-- the keys that only that browser can decrypt with. One row per browser profile; the endpoint is
-- unique, so a browser that signs in as somebody else moves its row. The row goes with the
-- session that made it: signing out, or a session that ends, stops the pushes.
CREATE TABLE push_subscriptions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    endpoint TEXT NOT NULL UNIQUE CHECK (length(endpoint) BETWEEN 1 AND 2048),
    p256dh TEXT NOT NULL CHECK (length(p256dh) BETWEEN 1 AND 200),
    auth TEXT NOT NULL CHECK (length(auth) BETWEEN 1 AND 100),
    -- What the user sees in the list of devices, e.g. "Firefox on Linux".
    label TEXT NOT NULL CHECK (length(label) <= 120),
    created_at INTEGER NOT NULL,
    -- How many pushes to it failed in a row. A push service that refuses the subscription again
    -- and again (not "gone", which removes it at once) will not take it later either: the server
    -- removes it after a few failures. A push that goes through sets the count back to zero.
    failures INTEGER NOT NULL DEFAULT 0 CHECK (failures >= 0)
);

CREATE INDEX push_subscriptions_user ON push_subscriptions (user_id);
-- For the cascade when a session is deleted.
CREATE INDEX push_subscriptions_session ON push_subscriptions (session_id);

-- Which events notify a user (push, and the sound and system notification of an open tab).
-- No row means everything is on. The in-app Inbox and the unread badges do not depend on it.
CREATE TABLE notification_prefs (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    direct_messages INTEGER NOT NULL DEFAULT 1 CHECK (direct_messages IN (0, 1)),
    chat_mentions INTEGER NOT NULL DEFAULT 1 CHECK (chat_mentions IN (0, 1)),
    thread_replies INTEGER NOT NULL DEFAULT 1 CHECK (thread_replies IN (0, 1)),
    -- Every message of a channel whose notify level is `all`.
    channel_messages INTEGER NOT NULL DEFAULT 1 CHECK (channel_messages IN (0, 1)),
    task_assigned INTEGER NOT NULL DEFAULT 1 CHECK (task_assigned IN (0, 1)),
    -- Mentions in task comments, pages and page comments.
    mentions INTEGER NOT NULL DEFAULT 1 CHECK (mentions IN (0, 1)),
    updated_at INTEGER NOT NULL
);

-- The server's VAPID key (a P-256 private key), made on first use. Push services use it to
-- tell that a push comes from the server the browser subscribed to.
CREATE TABLE push_keys (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    private_key BLOB NOT NULL CHECK (length(private_key) = 32),
    created_at INTEGER NOT NULL
);

-- Inbox notifications are pushed by a background service, so their writers stay as they are.
-- The notifications that exist now were seen (or not) long ago: they are not pushed.
ALTER TABLE notifications ADD COLUMN pushed_at INTEGER;
UPDATE notifications SET pushed_at = created_at;
CREATE INDEX notifications_unpushed ON notifications (id) WHERE pushed_at IS NULL;
