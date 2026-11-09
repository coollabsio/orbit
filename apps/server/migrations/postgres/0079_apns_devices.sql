-- iPhones that get pushes through Apple's push service (APNs), as browsers do in push_subscriptions.
-- A row belongs to the session that registered it: a revoked or expired session gets no pushes, and
-- a deleted session takes its devices with it.
CREATE TABLE apns_devices (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    -- The device token, lowercase hex. A phone has one: its row moves to whoever registers it last.
    token TEXT NOT NULL UNIQUE CHECK (length(token) BETWEEN 16 AND 200),
    -- `production` (App Store, TestFlight) or `sandbox` (development builds): which APNs host takes it.
    environment TEXT NOT NULL CHECK (environment IN ('production', 'sandbox')),
    -- What the user sees in the list of devices, e.g. "iPhone · iOS 26.1".
    label TEXT NOT NULL CHECK (length(label) <= 120),
    created_at BIGINT NOT NULL
);
CREATE INDEX apns_devices_user ON apns_devices (user_id);
-- For the cascade when a session is deleted.
CREATE INDEX apns_devices_session ON apns_devices (session_id);
