-- Mail, slice 1 (receive and read). SQLite: ../0056_mail.sql, which explains each table.
-- Same tables, columns, indexes and trigger messages; INTEGER is BIGINT, BLOB is BYTEA, and the
-- FTS5 table mail_search is a plain table with a generated tsvector column.

ALTER TABLE instance_settings ADD COLUMN mail_domains_enabled BIGINT NOT NULL DEFAULT 0
    CHECK (mail_domains_enabled IN (0, 1));

CREATE TABLE mailboxes (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('personal', 'shared')),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    owner_membership_id TEXT REFERENCES memberships(id) ON DELETE SET NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'suspended')),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CHECK (kind = 'personal' OR owner_membership_id IS NULL)
);

CREATE INDEX mailboxes_workspace ON mailboxes (workspace_id);
CREATE INDEX mailboxes_owner ON mailboxes (owner_membership_id)
    WHERE owner_membership_id IS NOT NULL;

CREATE TABLE mail_domains (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 253),
    status TEXT NOT NULL CHECK (status IN ('pending', 'verified', 'suspended', 'disabled')),
    verify_token TEXT NOT NULL,
    verified_at BIGINT,
    last_verified_at BIGINT,
    suspended_at BIGINT,
    catch_all_mailbox_id TEXT REFERENCES mailboxes(id) ON DELETE SET NULL,
    dns_report TEXT CHECK (dns_report IS NULL OR orbit_json_valid(dns_report)),
    checked_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX mail_domains_name ON mail_domains (lower(name));
CREATE INDEX mail_domains_workspace ON mail_domains (workspace_id);
CREATE INDEX mail_domains_catch_all ON mail_domains (catch_all_mailbox_id)
    WHERE catch_all_mailbox_id IS NOT NULL;
CREATE INDEX mail_domains_due ON mail_domains (status, checked_at);

CREATE TABLE mail_dkim_keys (
    id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES mail_domains(id) ON DELETE CASCADE,
    selector TEXT NOT NULL CHECK (length(selector) BETWEEN 1 AND 63),
    public_key TEXT NOT NULL,
    private_sealed BYTEA NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'retiring', 'retired')),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE (domain_id, selector)
);

CREATE TABLE mail_addresses (
    id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES mail_domains(id) ON DELETE CASCADE,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    local_part TEXT NOT NULL CHECK (length(local_part) BETWEEN 1 AND 64),
    kind TEXT NOT NULL CHECK (kind IN ('primary', 'alias')),
    created_at BIGINT NOT NULL,
    UNIQUE (domain_id, local_part)
);

CREATE INDEX mail_addresses_mailbox ON mail_addresses (mailbox_id);
CREATE UNIQUE INDEX mail_addresses_primary ON mail_addresses (mailbox_id) WHERE kind = 'primary';

CREATE FUNCTION mail_addresses_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM mail_domains
        JOIN mailboxes ON mailboxes.workspace_id = mail_domains.workspace_id
        WHERE mail_domains.id = NEW.domain_id AND mailboxes.id = NEW.mailbox_id
    )
    THEN
        RAISE EXCEPTION 'mail address must join a domain and a mailbox of one workspace';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER mail_addresses_scope_insert
    BEFORE INSERT ON mail_addresses
    FOR EACH ROW
    EXECUTE FUNCTION mail_addresses_scope_insert_fn();

CREATE TABLE mailbox_members (
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (mailbox_id, membership_id)
);

CREATE INDEX mailbox_members_membership ON mailbox_members (membership_id);

CREATE FUNCTION mailbox_members_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM mailboxes
        JOIN memberships ON memberships.workspace_id = mailboxes.workspace_id
        WHERE mailboxes.id = NEW.mailbox_id AND memberships.id = NEW.membership_id
    )
    THEN
        RAISE EXCEPTION 'mailbox member must belong to the mailbox''s workspace';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER mailbox_members_scope_insert
    BEFORE INSERT ON mailbox_members
    FOR EACH ROW
    EXECUTE FUNCTION mailbox_members_scope_insert_fn();

CREATE TABLE mail_keys (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    version BIGINT NOT NULL CHECK (version >= 1),
    key_sealed BYTEA NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, version)
);

CREATE TABLE mail_receipts (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (length(source) BETWEEN 1 AND 32),
    blob_id TEXT NOT NULL REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    key_version BIGINT NOT NULL CHECK (key_version >= 1),
    raw_sha256 TEXT NOT NULL CHECK (length(raw_sha256) = 64),
    size BIGINT NOT NULL CHECK (size >= 0),
    mail_from TEXT,
    peer_ip TEXT NOT NULL,
    helo TEXT NOT NULL,
    tls TEXT CHECK (tls IS NULL OR orbit_json_valid(tls)),
    auth TEXT NOT NULL CHECK (orbit_json_valid(auth)),
    status TEXT NOT NULL CHECK (status IN ('pending', 'ingested', 'failed')),
    attempts BIGINT NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    error TEXT,
    received_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX mail_receipts_status ON mail_receipts (workspace_id, status);
CREATE INDEX mail_receipts_blob ON mail_receipts (blob_id);

CREATE FUNCTION mail_receipts_scope_insert_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM attachment_blobs
        WHERE attachment_blobs.id = NEW.blob_id AND attachment_blobs.workspace_id = NEW.workspace_id
    )
    THEN
        RAISE EXCEPTION 'mail receipt scope mismatch';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER mail_receipts_scope_insert
    BEFORE INSERT ON mail_receipts
    FOR EACH ROW
    EXECUTE FUNCTION mail_receipts_scope_insert_fn();
CREATE FUNCTION mail_receipts_scope_immutable_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'mail receipt scope is immutable';
END;
$$;
CREATE TRIGGER mail_receipts_scope_immutable
    BEFORE UPDATE OF workspace_id, blob_id ON mail_receipts
    FOR EACH ROW
    WHEN (NEW.workspace_id <> OLD.workspace_id OR NEW.blob_id <> OLD.blob_id)
    EXECUTE FUNCTION mail_receipts_scope_immutable_fn();
CREATE FUNCTION mail_receipts_quarantine_blob_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE attachment_blobs
    SET quarantine_until = GREATEST(
        quarantine_until,
        floor(EXTRACT(EPOCH FROM clock_timestamp()))::BIGINT * 1000 + 86400000
    )
    WHERE id = OLD.blob_id;
    RETURN NULL;
END;
$$;
CREATE TRIGGER mail_receipts_quarantine_blob
    AFTER DELETE ON mail_receipts
    FOR EACH ROW
    EXECUTE FUNCTION mail_receipts_quarantine_blob_fn();

CREATE TABLE mail_receipt_recipients (
    receipt_id TEXT NOT NULL REFERENCES mail_receipts(id) ON DELETE CASCADE,
    address_id TEXT REFERENCES mail_addresses(id) ON DELETE SET NULL,
    rcpt TEXT NOT NULL CHECK (length(rcpt) BETWEEN 3 AND 320),
    PRIMARY KEY (receipt_id, rcpt)
);

CREATE INDEX mail_receipt_recipients_address ON mail_receipt_recipients (address_id)
    WHERE address_id IS NOT NULL;

CREATE TABLE mail_messages (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    receipt_id TEXT NOT NULL UNIQUE REFERENCES mail_receipts(id) ON DELETE CASCADE,
    raw_sha256 TEXT NOT NULL CHECK (length(raw_sha256) = 64),
    message_id_header TEXT,
    in_reply_to TEXT,
    reference_ids TEXT NOT NULL DEFAULT '[]' CHECK (orbit_json_valid(reference_ids)),
    from_name TEXT,
    from_addr TEXT,
    sender TEXT,
    reply_to TEXT NOT NULL DEFAULT '[]' CHECK (orbit_json_valid(reply_to)),
    to_addrs TEXT NOT NULL DEFAULT '[]' CHECK (orbit_json_valid(to_addrs)),
    cc_addrs TEXT NOT NULL DEFAULT '[]' CHECK (orbit_json_valid(cc_addrs)),
    subject TEXT NOT NULL DEFAULT '',
    sent_at BIGINT,
    received_at BIGINT NOT NULL,
    snippet TEXT NOT NULL DEFAULT '',
    body_text TEXT NOT NULL DEFAULT '',
    has_attachments BIGINT NOT NULL DEFAULT 0 CHECK (has_attachments IN (0, 1)),
    size BIGINT NOT NULL CHECK (size >= 0),
    warning TEXT,
    created_at BIGINT NOT NULL
);

CREATE INDEX mail_messages_message_id ON mail_messages (workspace_id, message_id_header);

CREATE TABLE mail_attachments (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES mail_messages(id) ON DELETE CASCADE,
    filename TEXT,
    declared_type TEXT NOT NULL,
    sniffed_type TEXT NOT NULL,
    size BIGINT NOT NULL CHECK (size >= 0),
    part_id BIGINT NOT NULL CHECK (part_id >= 0),
    content_id TEXT,
    disposition TEXT NOT NULL CHECK (disposition IN ('attachment', 'inline')),
    UNIQUE (message_id, part_id)
);

CREATE TABLE mail_threads (
    row_id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    subject TEXT NOT NULL DEFAULT '',
    normalized_subject TEXT NOT NULL DEFAULT '',
    folder TEXT NOT NULL CHECK (folder IN ('inbox', 'archive', 'spam', 'trash')),
    restore_folder TEXT CHECK (restore_folder IS NULL OR restore_folder IN ('inbox', 'archive', 'spam')),
    trashed_at BIGINT,
    spam_reason TEXT,
    last_message_at BIGINT NOT NULL,
    message_count BIGINT NOT NULL DEFAULT 0 CHECK (message_count >= 0),
    has_attachments BIGINT NOT NULL DEFAULT 0 CHECK (has_attachments IN (0, 1)),
    snippet TEXT NOT NULL DEFAULT '',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CHECK ((folder = 'trash') = (trashed_at IS NOT NULL))
);

CREATE INDEX mail_threads_folder ON mail_threads (mailbox_id, folder, last_message_at);
CREATE INDEX mail_threads_workspace ON mail_threads (workspace_id);
CREATE INDEX mail_threads_trashed ON mail_threads (trashed_at) WHERE trashed_at IS NOT NULL;

CREATE TABLE mail_thread_messages (
    thread_id TEXT NOT NULL REFERENCES mail_threads(id) ON DELETE CASCADE,
    message_id TEXT NOT NULL REFERENCES mail_messages(id) ON DELETE CASCADE,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    raw_sha256 TEXT NOT NULL CHECK (length(raw_sha256) = 64),
    external BIGINT NOT NULL DEFAULT 0 CHECK (external IN (0, 1)),
    created_at BIGINT NOT NULL,
    PRIMARY KEY (thread_id, message_id),
    UNIQUE (mailbox_id, raw_sha256)
);

CREATE INDEX mail_thread_messages_message ON mail_thread_messages (message_id);

CREATE TABLE mail_thread_user_state (
    thread_id TEXT NOT NULL REFERENCES mail_threads(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    last_read_at BIGINT,
    starred BIGINT NOT NULL DEFAULT 0 CHECK (starred IN (0, 1)),
    PRIMARY KEY (thread_id, membership_id)
);

CREATE INDEX mail_thread_user_state_membership ON mail_thread_user_state (membership_id);

CREATE TABLE mail_labels (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    color TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX mail_labels_name ON mail_labels (workspace_id, lower(name));

CREATE TABLE mail_thread_labels (
    thread_id TEXT NOT NULL REFERENCES mail_threads(id) ON DELETE CASCADE,
    label_id TEXT NOT NULL REFERENCES mail_labels(id) ON DELETE CASCADE,
    PRIMARY KEY (thread_id, label_id)
);

CREATE INDEX mail_thread_labels_label ON mail_thread_labels (label_id);

-- SQLite: CREATE VIRTUAL TABLE mail_search USING fts5(subject, people, body, content = '',
--     contentless_delete = 1, tokenize = 'unicode61 remove_diacritics 2'), keyed by
--     rowid = mail_threads.row_id. The server writes the rows; mail_search_delete removes them.
-- Subject lexemes weigh A, people B and body C.
CREATE TABLE mail_search (
    rowid BIGINT PRIMARY KEY,
    subject TEXT,
    people TEXT,
    body TEXT,
    search_vector tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('orbit_search', regexp_replace(coalesce(subject, ''), '[^[:alnum:]]+', ' ', 'g')), 'A')
        || setweight(to_tsvector('orbit_search', regexp_replace(coalesce(people, ''), '[^[:alnum:]]+', ' ', 'g')), 'B')
        || setweight(to_tsvector('orbit_search', regexp_replace(coalesce(body, ''), '[^[:alnum:]]+', ' ', 'g')), 'C')
    ) STORED
);
CREATE INDEX mail_search_search_vector ON mail_search USING GIN (search_vector);
CREATE FUNCTION mail_search_delete_fn() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM mail_search WHERE rowid = OLD.row_id;
    RETURN NULL;
END;
$$;
CREATE TRIGGER mail_search_delete
    AFTER DELETE ON mail_threads
    FOR EACH ROW
    EXECUTE FUNCTION mail_search_delete_fn();

CREATE TABLE mail_health_tests (
    id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES mail_domains(id) ON DELETE CASCADE,
    address_local TEXT NOT NULL CHECK (length(address_local) BETWEEN 1 AND 64),
    expires_at BIGINT NOT NULL,
    result TEXT CHECK (result IS NULL OR orbit_json_valid(result)),
    received_at BIGINT,
    created_at BIGINT NOT NULL,
    UNIQUE (domain_id, address_local)
);

CREATE TABLE mail_sender_prefs (
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    address TEXT NOT NULL CHECK (length(address) BETWEEN 3 AND 320),
    load_images BIGINT NOT NULL CHECK (load_images IN (0, 1)),
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (membership_id, address)
);
