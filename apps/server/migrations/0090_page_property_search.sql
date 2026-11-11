-- Docs search covers the property values of database rows. `pages.property_text` holds a row's
-- searchable values, one per line; the server computes it on every write that changes them.
-- NULL means not computed yet (a background job fills such rows) or not a row.
--
-- page_search gets a third column, `props`. FTS5 cannot add a column, so the index is dropped,
-- recreated and refilled under the same rowids (`page_search_rows` stays), and its triggers are
-- recreated to copy `property_text` (see 0027 for the rest).
ALTER TABLE pages ADD COLUMN property_text TEXT;

DROP TRIGGER page_search_insert;
DROP TRIGGER page_search_update;
DROP TRIGGER page_search_delete;
DROP TABLE page_search;

CREATE VIRTUAL TABLE page_search USING fts5(
    title,
    body,
    props,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO page_search (rowid, title, body, props)
SELECT
    page_search_rows.id,
    replace(replace(pages.title, char(2), ''), char(3), ''),
    replace(replace(pages.content_text, char(2), ''), char(3), ''),
    replace(replace(coalesce(pages.property_text, ''), char(2), ''), char(3), '')
FROM page_search_rows JOIN pages ON pages.id = page_search_rows.page_id;

CREATE TRIGGER page_search_insert
AFTER INSERT ON pages
BEGIN
    INSERT INTO page_search_rows (page_id) VALUES (NEW.id);
    INSERT INTO page_search (rowid, title, body, props)
    VALUES (
        (SELECT id FROM page_search_rows WHERE page_id = NEW.id),
        replace(replace(NEW.title, char(2), ''), char(3), ''),
        replace(replace(NEW.content_text, char(2), ''), char(3), ''),
        replace(replace(coalesce(NEW.property_text, ''), char(2), ''), char(3), '')
    );
END;

CREATE TRIGGER page_search_update
AFTER UPDATE OF title, content_text, property_text ON pages
WHEN NEW.title IS NOT OLD.title OR NEW.content_text IS NOT OLD.content_text
    OR NEW.property_text IS NOT OLD.property_text
BEGIN
    UPDATE page_search
    SET title = replace(replace(NEW.title, char(2), ''), char(3), ''),
        body = replace(replace(NEW.content_text, char(2), ''), char(3), ''),
        props = replace(replace(coalesce(NEW.property_text, ''), char(2), ''), char(3), '')
    WHERE rowid = (SELECT id FROM page_search_rows WHERE page_id = NEW.id);
END;

CREATE TRIGGER page_search_delete
AFTER DELETE ON pages
BEGIN
    DELETE FROM page_search
    WHERE rowid = (SELECT id FROM page_search_rows WHERE page_id = OLD.id);
    DELETE FROM page_search_rows WHERE page_id = OLD.id;
END;
