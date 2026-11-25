-- Postgres: the same change as apps/server/migrations/20261125075245_frame_origins.sql
ALTER TABLE instance_settings ADD COLUMN frame_origins TEXT NOT NULL DEFAULT '';
