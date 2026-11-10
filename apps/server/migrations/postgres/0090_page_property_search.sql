-- Docs search covers the property values of database rows. `pages.property_text` holds a row's
-- searchable values, one per line; the server computes it on every write that changes them.
-- NULL means not computed yet (a background job fills such rows) or not a row.
--
-- page_search gets `props`, indexed at weight C (title A, body B); the generated search vector
-- is dropped and added again with it, and the triggers copy `property_text`.
ALTER TABLE pages ADD COLUMN property_text TEXT;

ALTER TABLE page_search ADD COLUMN props TEXT;
DROP INDEX page_search_search_vector;
ALTER TABLE page_search DROP COLUMN search_vector;
ALTER TABLE page_search ADD COLUMN search_vector tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('orbit_search', regexp_replace(coalesce(title, ''), '[^[:alnum:]]+', ' ', 'g')), 'A')
    || setweight(to_tsvector('orbit_search', regexp_replace(coalesce(body, ''), '[^[:alnum:]]+', ' ', 'g')), 'B')
    || setweight(to_tsvector('orbit_search', regexp_replace(coalesce(props, ''), '[^[:alnum:]]+', ' ', 'g')), 'C')
) STORED;
CREATE INDEX page_search_search_vector ON page_search USING GIN (search_vector);

CREATE OR REPLACE FUNCTION page_search_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO page_search_rows (page_id) VALUES (NEW.id);
    INSERT INTO page_search (rowid, title, body, props)
    VALUES (
        (SELECT id FROM page_search_rows WHERE page_id = NEW.id),
        replace(replace(NEW.title, chr(2), ''), chr(3), ''),
        replace(replace(NEW.content_text, chr(2), ''), chr(3), ''),
        replace(replace(coalesce(NEW.property_text, ''), chr(2), ''), chr(3), '')
    );
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION page_search_update_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE page_search
    SET title = replace(replace(NEW.title, chr(2), ''), chr(3), ''),
        body = replace(replace(NEW.content_text, chr(2), ''), chr(3), ''),
        props = replace(replace(coalesce(NEW.property_text, ''), chr(2), ''), chr(3), '')
    WHERE rowid = (SELECT id FROM page_search_rows WHERE page_id = NEW.id);
    RETURN NULL;
END;
$$;

DROP TRIGGER page_search_update ON pages;
CREATE TRIGGER page_search_update
    AFTER UPDATE OF title, content_text, property_text ON pages
    FOR EACH ROW
    WHEN (NEW.title IS DISTINCT FROM OLD.title OR NEW.content_text IS DISTINCT FROM OLD.content_text
        OR NEW.property_text IS DISTINCT FROM OLD.property_text)
    EXECUTE FUNCTION page_search_update_fn();
