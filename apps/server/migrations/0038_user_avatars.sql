-- Profile pictures. The web app crops and scales the image (256 px square) before upload, so the
-- bytes are small and live in the database: one row per user, replaced on each upload.
-- The server accepts PNG, JPEG and WebP only, checked by magic bytes (apps/server/src/auth_routes.rs).
-- `updated_at` versions the image URL, so browsers can cache it for good.
-- Users are never hard-deleted, so the user reference needs no ON DELETE action.
CREATE TABLE user_avatars (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp')),
    bytes BLOB NOT NULL CHECK (length(bytes) BETWEEN 1 AND 524288),
    updated_at INTEGER NOT NULL
);
