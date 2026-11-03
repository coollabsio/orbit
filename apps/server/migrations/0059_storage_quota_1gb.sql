-- The default storage quota per workspace is 1 GB instead of 5 GB. Installations that kept the old default get the
-- new one; a quota the admin chose stays. New installations get the row from 0042 and the 5 GB column default from
-- 0053, so this update gives them 1 GB too.
UPDATE instance_settings SET workspace_storage_quota_bytes = 1000000000 WHERE workspace_storage_quota_bytes = 5000000000;
