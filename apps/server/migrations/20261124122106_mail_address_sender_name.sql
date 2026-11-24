-- SQLite
-- The name recipients see on mail from this address. NULL: the mailbox name.
ALTER TABLE mail_addresses ADD COLUMN sender_name TEXT;
