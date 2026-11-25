-- SQLite
-- Which URLs workspace admins may use for custom pages (#94): any https site, only the saved frame_origins, or none.
-- ORBIT_FRAME_ORIGINS replaces it.
ALTER TABLE instance_settings ADD COLUMN custom_page_mode TEXT NOT NULL DEFAULT 'any'
    CHECK (custom_page_mode IN ('any', 'listed', 'off'));
