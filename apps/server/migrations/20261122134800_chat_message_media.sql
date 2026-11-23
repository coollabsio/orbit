-- SQLite
-- A chat message can carry a GIF or a sticker from KLIPY: its CDN URL, size and title. Null `media_kind` is no media.
ALTER TABLE chat_messages ADD COLUMN media_kind TEXT CHECK (media_kind IN ('gif', 'sticker'));
ALTER TABLE chat_messages ADD COLUMN media_url TEXT;
ALTER TABLE chat_messages ADD COLUMN media_width BIGINT;
ALTER TABLE chat_messages ADD COLUMN media_height BIGINT;
ALTER TABLE chat_messages ADD COLUMN media_title TEXT;
-- The KLIPY API key that the root user saves in Admin, encrypted under the app key (`ORBIT_KLIPY_API_KEY` wins).
ALTER TABLE instance_settings ADD COLUMN klipy_api_key BLOB;
