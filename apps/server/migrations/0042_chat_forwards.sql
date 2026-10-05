-- Forwarded chat messages.

-- A forward is a new message in the destination conversation, written by the member who
-- forwards. It is a snapshot: its `body` is a copy of the original's body, and its files are
-- new `chat_message_files` rows that point at the same blobs. An edit or the deletion of the
-- original changes nothing in it.
-- The four columns say where the copy came from; all four are set on a forward and NULL on
-- every other message. A forward of a forward keeps the values of the first original.
-- No foreign keys: the original message (and its conversation) can be hard-deleted, and the
-- forward keeps the ids. Users are never hard-deleted, but the author is read from this row
-- with no join, so it needs no reference.
-- No index: nothing lists the forwards of a message.

-- The original message.
ALTER TABLE chat_messages ADD COLUMN forward_of_id TEXT;
-- The conversation of the original message.
ALTER TABLE chat_messages ADD COLUMN forward_conversation_id TEXT;
-- The author of the original message.
ALTER TABLE chat_messages ADD COLUMN forward_author_id TEXT;
-- When the original message was sent, in milliseconds.
ALTER TABLE chat_messages ADD COLUMN forward_created_at INTEGER;
