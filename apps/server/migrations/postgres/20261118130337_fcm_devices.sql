-- Android phones that get pushes through Firebase Cloud Messaging (FCM), as iPhones do in apns_devices.
-- A row belongs to the session that registered it: a revoked or expired session gets no pushes, and
-- a deleted session takes its devices with it.
CREATE TABLE fcm_devices (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    -- The FCM registration token. A phone has one: its row moves to whoever registers it last.
    token TEXT NOT NULL UNIQUE CHECK (length(token) BETWEEN 32 AND 1024),
    -- What the user sees in the list of devices, e.g. "Pixel 9 · Android 16".
    label TEXT NOT NULL CHECK (length(label) <= 120),
    created_at BIGINT NOT NULL
);
CREATE INDEX fcm_devices_user ON fcm_devices (user_id);
-- For the cascade when a session is deleted.
CREATE INDEX fcm_devices_session ON fcm_devices (session_id);
