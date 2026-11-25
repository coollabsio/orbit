-- Postgres: the same change as apps/server/migrations/20261125103650_custom_page_mode.sql
ALTER TABLE instance_settings ADD COLUMN custom_page_mode TEXT NOT NULL DEFAULT 'any'
    CHECK (custom_page_mode IN ('any', 'listed', 'off'));
