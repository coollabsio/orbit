-- Postgres: the same change as apps/server/migrations/20261125093246_chat_message_max_chars.sql
-- The most characters a chat message can have (Admin → Limits). Messages that exist are not changed.
ALTER TABLE instance_settings ADD COLUMN chat_message_max_chars BIGINT NOT NULL DEFAULT 20000;
