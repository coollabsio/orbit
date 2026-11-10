-- The bucket that may hold the installation's attachment files (`blobs/`): set when attachments go to S3, cleared
-- when the mover finds none left in it. Orbit refuses to start with another bucket while files may be in this one,
-- also when the bucket comes from `ORBIT_S3_*`, which is never saved. Only the location: no credentials.
ALTER TABLE instance_settings ADD COLUMN files_s3_endpoint TEXT;
ALTER TABLE instance_settings ADD COLUMN files_s3_bucket TEXT;
ALTER TABLE instance_settings ADD COLUMN files_s3_prefix TEXT;

-- Before this version, files could be only in the bucket saved in Admin. The mover clears an empty one.
UPDATE instance_settings
SET files_s3_endpoint = s3_endpoint, files_s3_bucket = s3_bucket, files_s3_prefix = s3_prefix
WHERE s3_endpoint IS NOT NULL AND s3_bucket IS NOT NULL;
