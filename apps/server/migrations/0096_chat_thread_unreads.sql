-- Unreads shows the replies of a thread a member has no state in (no `chat_thread_members` row):
-- the replies newer than both the member's `joined_at` and this message id are unread. Members who
-- were there before this migration start with nothing unread: the newest message at that time.
ALTER TABLE chat_members ADD COLUMN threads_read_to TEXT;
UPDATE chat_members SET threads_read_to = (SELECT MAX(id) FROM chat_messages);
