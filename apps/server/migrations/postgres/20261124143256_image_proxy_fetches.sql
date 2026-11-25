-- Postgres: the same change as apps/server/migrations/20261124143256_image_proxy_fetches.sql
ALTER TABLE instance_settings ADD COLUMN image_proxy_fetches BIGINT NOT NULL DEFAULT 32;
