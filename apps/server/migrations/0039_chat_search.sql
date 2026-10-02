-- Chat full-text search: an FTS5 index over message bodies, with `unicode61 remove_diacritics 2`
-- as Docs search has. It is an external-content index: the text stays in `chat_messages` only,
-- and the index refers to a message by `row_id` (an INTEGER PRIMARY KEY, which a backup's
-- VACUUM INTO cannot renumber).
--
-- Triggers keep it in sync, in the transaction of the message. Only rows of kind 'message' are
-- indexed. An external-content index must be told the old text to take a row out, which is
-- what the 'delete' command does.
-- The server strips U+0002 and U+0003 from a body before it is stored: search uses them as
-- highlight markers.
CREATE VIRTUAL TABLE chat_search USING fts5(
    body,
    content = 'chat_messages',
    content_rowid = 'row_id',
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO chat_search (rowid, body)
SELECT row_id, body FROM chat_messages WHERE kind = 'message';

CREATE TRIGGER chat_search_insert
AFTER INSERT ON chat_messages
WHEN NEW.kind = 'message'
BEGIN
    INSERT INTO chat_search (rowid, body) VALUES (NEW.row_id, NEW.body);
END;

CREATE TRIGGER chat_search_update
AFTER UPDATE OF body ON chat_messages
WHEN NEW.kind = 'message' AND NEW.body IS NOT OLD.body
BEGIN
    INSERT INTO chat_search (chat_search, rowid, body) VALUES ('delete', OLD.row_id, OLD.body);
    INSERT INTO chat_search (rowid, body) VALUES (NEW.row_id, NEW.body);
END;

-- Fires for direct deletes and for the thread, conversation and workspace cascades alike.
CREATE TRIGGER chat_search_delete
AFTER DELETE ON chat_messages
WHEN OLD.kind = 'message'
BEGIN
    INSERT INTO chat_search (chat_search, rowid, body) VALUES ('delete', OLD.row_id, OLD.body);
END;
