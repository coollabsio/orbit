-- An admin can delete a stored file (a blob); its references stay so they can show "Deleted file".
ALTER TABLE attachment_blobs ADD COLUMN deleted_at BIGINT;
