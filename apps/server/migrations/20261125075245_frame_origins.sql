-- SQLite
-- The custom page origins (#94) that the root user saves in Admin, one per line. ORBIT_FRAME_ORIGINS replaces them.
ALTER TABLE instance_settings ADD COLUMN frame_origins TEXT NOT NULL DEFAULT '';
