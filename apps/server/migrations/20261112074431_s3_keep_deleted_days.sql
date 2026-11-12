-- How many days a deleted or replaced attachment file stays in S3 before Orbit deletes it. S3 files are not in
-- backups, so restoring a backup older than this can miss files.
ALTER TABLE instance_settings ADD COLUMN s3_keep_deleted_days BIGINT NOT NULL DEFAULT 40;
