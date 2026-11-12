-- As in apps/server/migrations/20261112074431_s3_keep_deleted_days.sql.
ALTER TABLE instance_settings ADD COLUMN s3_keep_deleted_days BIGINT NOT NULL DEFAULT 40;
