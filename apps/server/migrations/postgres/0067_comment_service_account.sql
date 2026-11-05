-- The service account that wrote a comment through an API token. `author_id` stays the member
-- the write was checked as, so existing readers keep a user.
ALTER TABLE task_comments ADD COLUMN author_service_account_id TEXT;
