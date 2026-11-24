-- SQLite
-- Outside images that one server loads at once for the image proxy (Admin → Limits).
ALTER TABLE instance_settings ADD COLUMN image_proxy_fetches BIGINT NOT NULL DEFAULT 32;
