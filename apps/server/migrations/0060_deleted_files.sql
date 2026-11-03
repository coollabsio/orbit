-- An admin can delete a stored file (a blob) from Settings → General → Storage. Its references stay, so tasks,
-- pages and chat messages can show "Deleted file". The row then keeps no bytes: `deleted_at` is set, `sha256` and
-- `storage_key` become `deleted:<id>` (so the same bytes can be uploaded again as a new blob, and reconcile removes
-- the old object as untracked), and the file stops counting toward the workspace's storage.
ALTER TABLE attachment_blobs ADD COLUMN deleted_at INTEGER;
