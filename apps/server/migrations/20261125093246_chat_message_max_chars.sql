-- SQLite
-- The most characters a chat message can have (Admin → Limits). Messages that exist are not changed.
ALTER TABLE instance_settings ADD COLUMN chat_message_max_chars BIGINT NOT NULL DEFAULT 20000;
