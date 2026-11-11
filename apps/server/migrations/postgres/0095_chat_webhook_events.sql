-- Two-way integrations: with `events` on, Orbit sends each member's message in the webhook's channel,
-- signed, to its request URL, and the webhook's secret can also read the channel
-- (apps/server/src/chat_routes/events.rs).
ALTER TABLE chat_webhooks ADD COLUMN events BIGINT NOT NULL DEFAULT 0 CHECK (events IN (0, 1));
