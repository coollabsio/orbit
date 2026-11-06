-- Layout blocks of an integration's message (apps/server/src/repositories/chat/blocks.rs): a JSON array,
-- checked and normalized by the server. `body` stays the plain text of the message.
ALTER TABLE chat_messages ADD COLUMN blocks TEXT;
