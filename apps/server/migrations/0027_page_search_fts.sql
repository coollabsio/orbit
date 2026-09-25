-- Docs full-text search: an FTS5 index over page titles and body text (`content_text`), with
-- `unicode61 remove_diacritics 2` so "uber" and "über" both find "Über". Triggers on `pages`
-- keep it in sync. It holds trashed pages too (queries only return live, visible pages), so
-- trash and restore need no index work.
--
-- FTS rowids come from `page_search_rows`, not from the implicit rowid of `pages`: `pages` has a
-- TEXT primary key, and VACUUM (backups use VACUUM INTO) may renumber implicit rowids, which
-- would silently point index rows at other pages. An INTEGER PRIMARY KEY is stable.
-- The control characters U+0002/U+0003 are stripped from indexed text: search uses them as
-- highlight markers in snippet()/highlight() output.
CREATE TABLE page_search_rows (
    id INTEGER PRIMARY KEY,
    page_id TEXT NOT NULL UNIQUE
);

CREATE VIRTUAL TABLE page_search USING fts5(
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO page_search_rows (page_id) SELECT id FROM pages ORDER BY created_at, id;

INSERT INTO page_search (rowid, title, body)
SELECT
    page_search_rows.id,
    replace(replace(pages.title, char(2), ''), char(3), ''),
    replace(replace(pages.content_text, char(2), ''), char(3), '')
FROM page_search_rows JOIN pages ON pages.id = page_search_rows.page_id;

CREATE TRIGGER page_search_insert
AFTER INSERT ON pages
BEGIN
    INSERT INTO page_search_rows (page_id) VALUES (NEW.id);
    INSERT INTO page_search (rowid, title, body)
    VALUES (
        (SELECT id FROM page_search_rows WHERE page_id = NEW.id),
        replace(replace(NEW.title, char(2), ''), char(3), ''),
        replace(replace(NEW.content_text, char(2), ''), char(3), '')
    );
END;

CREATE TRIGGER page_search_update
AFTER UPDATE OF title, content_text ON pages
WHEN NEW.title IS NOT OLD.title OR NEW.content_text IS NOT OLD.content_text
BEGIN
    UPDATE page_search
    SET title = replace(replace(NEW.title, char(2), ''), char(3), ''),
        body = replace(replace(NEW.content_text, char(2), ''), char(3), '')
    WHERE rowid = (SELECT id FROM page_search_rows WHERE page_id = NEW.id);
END;

-- Fires for direct deletes (trash purge) and for the teamspace/workspace/owner cascades alike.
CREATE TRIGGER page_search_delete
AFTER DELETE ON pages
BEGIN
    DELETE FROM page_search
    WHERE rowid = (SELECT id FROM page_search_rows WHERE page_id = OLD.id);
    DELETE FROM page_search_rows WHERE page_id = OLD.id;
END;
