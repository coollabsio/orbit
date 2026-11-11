-- Mail, slice 1 (receive and read): domains with DNS verification and DKIM keys, mailboxes with
-- their addresses and members, the encrypted raw messages as received (receipts), the parsed
-- messages, threads for each mailbox, labels, each member's state, search, the instance health
-- test addresses and per-sender preferences.
--
-- * Times are milliseconds since the epoch; flags are 0/1 INTEGER.
-- * No HTML is stored: it is sanitized from the raw message on each view.
-- * The raw message is an ordinary blob (`attachment_blobs`), sealed with the workspace data key.
--   `mail_receipts.blob_id` is a blob reference like `chat_message_files.blob_id`, so the blob
--   reconcile deletes a blob once no receipt uses it.
-- * Users are never hard-deleted. A removed member's rows go with the membership, except a
--   personal mailbox, which stays (its owner set to NULL) for the admins.

-- Instance switch (Admin, root user): whether workspaces may add mail domains.
ALTER TABLE instance_settings ADD COLUMN mail_domains_enabled INTEGER NOT NULL DEFAULT 0
    CHECK (mail_domains_enabled IN (0, 1));

-- Instance switch: whether workspace admins may set up their own mail providers (Orbit Cloud: 0).
ALTER TABLE instance_settings ADD COLUMN mail_workspace_providers_enabled INTEGER NOT NULL DEFAULT 1
    CHECK (mail_workspace_providers_enabled IN (0, 1));

-- A mail provider that receives mail for a domain and delivers it to Orbit over HTTPS
-- (`/api/v1/mail/inbound/<kind>/<id>`). No workspace: an instance provider of the root user, which
-- every workspace may pick; else a provider that only its workspace may pick.
CREATE TABLE mail_providers (
    id TEXT PRIMARY KEY,
    workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('cloudflare', 'postmark', 'mailgun', 'resend', 'ses')),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    -- JSON of the kind's secrets, sealed with the app key; associated data `mail-provider:<id>`.
    secrets BLOB NOT NULL,
    -- MX hosts, region, bucket and prefix (JSON).
    settings TEXT NOT NULL CHECK (json_valid(settings)),
    last_received_at INTEGER,
    last_error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX mail_providers_workspace ON mail_providers (workspace_id);

CREATE TABLE mailboxes (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('personal', 'shared')),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    -- The member a personal mailbox belongs to (also its only member); NULL for a shared one.
    -- NULL as well once the owner left the workspace: the server suspends the mailbox in the
    -- same transaction, and it stays for the admins.
    owner_membership_id TEXT REFERENCES memberships(id) ON DELETE SET NULL,
    -- A suspended mailbox refuses mail (`550 5.2.1`).
    status TEXT NOT NULL CHECK (status IN ('active', 'suspended')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (kind = 'personal' OR owner_membership_id IS NULL)
);

CREATE INDEX mailboxes_workspace ON mailboxes (workspace_id);
CREATE INDEX mailboxes_owner ON mailboxes (owner_membership_id)
    WHERE owner_membership_id IS NOT NULL;

CREATE TABLE mail_domains (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- Normalized by the server: lower case, IDNA A-label, no trailing dot.
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 253),
    status TEXT NOT NULL CHECK (status IN ('pending', 'verified', 'suspended', 'disabled')),
    -- Expected in the TXT record at `_orbit-verify.<name>`. A re-added domain gets a new one.
    verify_token TEXT NOT NULL,
    verified_at INTEGER,
    last_verified_at INTEGER,
    -- When a verified domain lost its TXT record; mail is still accepted for a grace period.
    suspended_at INTEGER,
    -- Receives mail for unknown local parts of the domain.
    catch_all_mailbox_id TEXT REFERENCES mailboxes(id) ON DELETE SET NULL,
    -- The last DNS check: a result for each record (verify, mx, dkim, spf, dmarc).
    dns_report TEXT CHECK (dns_report IS NULL OR json_valid(dns_report)),
    checked_at INTEGER,
    -- The provider that receives the domain's mail; NULL: Orbit's own SMTP listener. A provider in
    -- use cannot be deleted (NO ACTION, so a workspace's cascade still removes both).
    provider_id TEXT REFERENCES mail_providers(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX mail_domains_provider ON mail_domains (provider_id) WHERE provider_id IS NOT NULL;

-- A domain may use an instance provider or a provider of its own workspace.
CREATE TRIGGER mail_domains_provider_scope_insert
BEFORE INSERT ON mail_domains
WHEN NEW.provider_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM mail_providers
    WHERE id = NEW.provider_id AND (workspace_id IS NULL OR workspace_id = NEW.workspace_id))
BEGIN SELECT RAISE(ABORT, 'mail domain must use a provider of its workspace'); END;

CREATE TRIGGER mail_domains_provider_scope_update
BEFORE UPDATE OF provider_id, workspace_id ON mail_domains
WHEN NEW.provider_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM mail_providers
    WHERE id = NEW.provider_id AND (workspace_id IS NULL OR workspace_id = NEW.workspace_id))
BEGIN SELECT RAISE(ABORT, 'mail domain must use a provider of its workspace'); END;

-- One owner for each name on the whole instance.
CREATE UNIQUE INDEX mail_domains_name ON mail_domains (lower(name));
CREATE INDEX mail_domains_workspace ON mail_domains (workspace_id);
CREATE INDEX mail_domains_catch_all ON mail_domains (catch_all_mailbox_id)
    WHERE catch_all_mailbox_id IS NOT NULL;
CREATE INDEX mail_domains_due ON mail_domains (status, checked_at);

CREATE TABLE mail_dkim_keys (
    id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES mail_domains(id) ON DELETE CASCADE,
    selector TEXT NOT NULL CHECK (length(selector) BETWEEN 1 AND 63),
    -- Base64 DER public key, as in the `p=` tag of the DNS record.
    public_key TEXT NOT NULL,
    -- The private key sealed with the app key; associated data `dkim:<domain_id>:<selector>`.
    private_sealed BLOB NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'retiring', 'retired')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (domain_id, selector)
);

CREATE TABLE mail_addresses (
    id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES mail_domains(id) ON DELETE CASCADE,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    -- Lower case, without a `+tag`.
    local_part TEXT NOT NULL CHECK (length(local_part) BETWEEN 1 AND 64),
    kind TEXT NOT NULL CHECK (kind IN ('primary', 'alias')),
    created_at INTEGER NOT NULL,
    UNIQUE (domain_id, local_part)
);

CREATE INDEX mail_addresses_mailbox ON mail_addresses (mailbox_id);
CREATE UNIQUE INDEX mail_addresses_primary ON mail_addresses (mailbox_id) WHERE kind = 'primary';

CREATE TRIGGER mail_addresses_scope_insert
BEFORE INSERT ON mail_addresses
WHEN NOT EXISTS (
    SELECT 1 FROM mail_domains
    JOIN mailboxes ON mailboxes.workspace_id = mail_domains.workspace_id
    WHERE mail_domains.id = NEW.domain_id AND mailboxes.id = NEW.mailbox_id
)
BEGIN
    SELECT RAISE(ABORT, 'mail address must join a domain and a mailbox of one workspace');
END;

CREATE TABLE mailbox_members (
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (mailbox_id, membership_id)
) WITHOUT ROWID;

CREATE INDEX mailbox_members_membership ON mailbox_members (membership_id);

CREATE TRIGGER mailbox_members_scope_insert
BEFORE INSERT ON mailbox_members
WHEN NOT EXISTS (
    SELECT 1 FROM mailboxes
    JOIN memberships ON memberships.workspace_id = mailboxes.workspace_id
    WHERE mailboxes.id = NEW.mailbox_id AND memberships.id = NEW.membership_id
)
BEGIN
    SELECT RAISE(ABORT, 'mailbox member must belong to the mailbox''s workspace');
END;

-- The workspace data keys: 32 random bytes sealed with the app key, associated data
-- `mail-key:<workspace_id>`. Old versions stay, so older receipts still open.
CREATE TABLE mail_keys (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    version INTEGER NOT NULL CHECK (version >= 1),
    key_sealed BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, version)
) WITHOUT ROWID;

-- A message as received for one workspace, stored before the `250` reply. The ingest job parses
-- it into `mail_messages` and threads.
CREATE TABLE mail_receipts (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- How it arrived (`smtp`; later other adapters).
    source TEXT NOT NULL CHECK (length(source) BETWEEN 1 AND 32),
    -- The raw message sealed with data key `key_version`; associated data `mail-raw:<id>`.
    blob_id TEXT NOT NULL REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    key_version INTEGER NOT NULL CHECK (key_version >= 1),
    -- SHA-256 (hex) of the raw message before sealing: the dedupe key in a mailbox.
    raw_sha256 TEXT NOT NULL CHECK (length(raw_sha256) = 64),
    size INTEGER NOT NULL CHECK (size >= 0),
    -- NULL for the null sender `<>`.
    mail_from TEXT,
    peer_ip TEXT NOT NULL,
    helo TEXT NOT NULL,
    tls TEXT CHECK (tls IS NULL OR json_valid(tls)),
    -- SPF, DKIM and DMARC results and the aligned domain.
    auth TEXT NOT NULL CHECK (json_valid(auth)),
    status TEXT NOT NULL CHECK (status IN ('pending', 'ingested', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    error TEXT,
    received_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX mail_receipts_status ON mail_receipts (workspace_id, status);
CREATE INDEX mail_receipts_blob ON mail_receipts (blob_id);

CREATE TRIGGER mail_receipts_scope_insert
BEFORE INSERT ON mail_receipts
WHEN NOT EXISTS (
    SELECT 1 FROM attachment_blobs
    WHERE attachment_blobs.id = NEW.blob_id AND attachment_blobs.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'mail receipt scope mismatch');
END;

CREATE TRIGGER mail_receipts_scope_immutable
BEFORE UPDATE OF workspace_id, blob_id ON mail_receipts
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.blob_id <> OLD.blob_id
BEGIN
    SELECT RAISE(ABORT, 'mail receipt scope is immutable');
END;

-- Fires for direct deletes and for the workspace cascade alike.
CREATE TRIGGER mail_receipts_quarantine_blob
AFTER DELETE ON mail_receipts
BEGIN
    UPDATE attachment_blobs
    SET quarantine_until = MAX(
        quarantine_until,
        CAST(strftime('%s', 'now') AS INTEGER) * 1000 + 86400000
    )
    WHERE id = OLD.blob_id;
END;

CREATE TABLE mail_receipt_recipients (
    receipt_id TEXT NOT NULL REFERENCES mail_receipts(id) ON DELETE CASCADE,
    -- NULL once the address is removed; the ingest then skips this recipient.
    address_id TEXT REFERENCES mail_addresses(id) ON DELETE SET NULL,
    -- The address as sent in `RCPT TO`, with its `+tag`.
    rcpt TEXT NOT NULL CHECK (length(rcpt) BETWEEN 3 AND 320),
    PRIMARY KEY (receipt_id, rcpt)
) WITHOUT ROWID;

CREATE INDEX mail_receipt_recipients_address ON mail_receipt_recipients (address_id)
    WHERE address_id IS NOT NULL;

-- A parsed message, once for each receipt. Its bytes (attachments, HTML) are read from the
-- receipt's blob, so it goes with its receipt. Address lists are JSON arrays of
-- `{"name": …, "address": …}`.
CREATE TABLE mail_messages (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    receipt_id TEXT NOT NULL UNIQUE REFERENCES mail_receipts(id) ON DELETE CASCADE,
    raw_sha256 TEXT NOT NULL CHECK (length(raw_sha256) = 64),
    message_id_header TEXT,
    in_reply_to TEXT,
    -- The `References` header: up to 100 message ids (JSON array of strings).
    reference_ids TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(reference_ids)),
    from_name TEXT,
    from_addr TEXT,
    sender TEXT,
    reply_to TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(reply_to)),
    to_addrs TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(to_addrs)),
    cc_addrs TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(cc_addrs)),
    subject TEXT NOT NULL DEFAULT '',
    -- The `Date` header, when it parses.
    sent_at INTEGER,
    received_at INTEGER NOT NULL,
    snippet TEXT NOT NULL DEFAULT '',
    body_text TEXT NOT NULL DEFAULT '',
    has_attachments INTEGER NOT NULL DEFAULT 0 CHECK (has_attachments IN (0, 1)),
    size INTEGER NOT NULL CHECK (size >= 0),
    -- mail-parser's index of the HTML body part, when there is one (rendered on each view).
    html_part INTEGER CHECK (html_part IS NULL OR html_part >= 0),
    -- Why the message shows as plain text only (`parse_failed`, `limit`).
    warning TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX mail_messages_message_id ON mail_messages (workspace_id, message_id_header);

CREATE TABLE mail_attachments (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES mail_messages(id) ON DELETE CASCADE,
    filename TEXT,
    -- The Content-Type of the part, and the type detected from its bytes (served).
    declared_type TEXT NOT NULL,
    sniffed_type TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size >= 0),
    -- mail-parser's index of the part in the raw message.
    part_id INTEGER NOT NULL CHECK (part_id >= 0),
    content_id TEXT,
    disposition TEXT NOT NULL CHECK (disposition IN ('attachment', 'inline')),
    UNIQUE (message_id, part_id)
);

CREATE TABLE mail_threads (
    -- Stable through VACUUM INTO (backups); the search index refers to it.
    row_id INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    subject TEXT NOT NULL DEFAULT '',
    -- Lower case without `Re:`/`Fwd:` prefixes; a reply joins only on a match or a known sender.
    normalized_subject TEXT NOT NULL DEFAULT '',
    folder TEXT NOT NULL CHECK (folder IN ('inbox', 'archive', 'spam', 'trash')),
    -- The folder before trash, for Restore (inbox when NULL).
    restore_folder TEXT CHECK (restore_folder IS NULL OR restore_folder IN ('inbox', 'archive', 'spam')),
    -- Purged 30 days later.
    trashed_at INTEGER,
    spam_reason TEXT,
    last_message_at INTEGER NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 0 CHECK (message_count >= 0),
    has_attachments INTEGER NOT NULL DEFAULT 0 CHECK (has_attachments IN (0, 1)),
    snippet TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK ((folder = 'trash') = (trashed_at IS NOT NULL))
);

CREATE INDEX mail_threads_folder ON mail_threads (mailbox_id, folder, last_message_at);
CREATE INDEX mail_threads_workspace ON mail_threads (workspace_id);
CREATE INDEX mail_threads_trashed ON mail_threads (trashed_at) WHERE trashed_at IS NOT NULL;

-- A message in a thread of one mailbox. A message to several mailboxes is in one thread of each.
CREATE TABLE mail_thread_messages (
    thread_id TEXT NOT NULL REFERENCES mail_threads(id) ON DELETE CASCADE,
    message_id TEXT NOT NULL REFERENCES mail_messages(id) ON DELETE CASCADE,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    raw_sha256 TEXT NOT NULL CHECK (length(raw_sha256) = 64),
    -- From outside the workspace's domains into a thread that started inside them.
    external INTEGER NOT NULL DEFAULT 0 CHECK (external IN (0, 1)),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (thread_id, message_id),
    -- The dedupe key: the same raw message is delivered to a mailbox once.
    UNIQUE (mailbox_id, raw_sha256)
);

CREATE INDEX mail_thread_messages_message ON mail_thread_messages (message_id);

CREATE TABLE mail_thread_user_state (
    thread_id TEXT NOT NULL REFERENCES mail_threads(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    -- Unread while this is NULL or before the thread's `last_message_at`.
    last_read_at INTEGER,
    starred INTEGER NOT NULL DEFAULT 0 CHECK (starred IN (0, 1)),
    PRIMARY KEY (thread_id, membership_id)
) WITHOUT ROWID;

CREATE INDEX mail_thread_user_state_membership ON mail_thread_user_state (membership_id);

CREATE TABLE mail_labels (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    color TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX mail_labels_name ON mail_labels (workspace_id, lower(name));

CREATE TABLE mail_thread_labels (
    thread_id TEXT NOT NULL REFERENCES mail_threads(id) ON DELETE CASCADE,
    label_id TEXT NOT NULL REFERENCES mail_labels(id) ON DELETE CASCADE,
    PRIMARY KEY (thread_id, label_id)
) WITHOUT ROWID;

CREATE INDEX mail_thread_labels_label ON mail_thread_labels (label_id);

-- Thread search, keyed by rowid = mail_threads.row_id: the subject, the names and addresses of
-- the people, and the first 256 KB of the bodies. Contentless (the text is in mail_messages);
-- contentless_delete lets the server replace a thread's row with DELETE + INSERT.
CREATE VIRTUAL TABLE mail_search USING fts5(
    subject,
    people,
    body,
    content = '',
    contentless_delete = 1,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Fires for direct deletes and for the mailbox and workspace cascades alike.
CREATE TRIGGER mail_search_delete
AFTER DELETE ON mail_threads
BEGIN
    DELETE FROM mail_search WHERE rowid = OLD.row_id;
END;

-- Mail that a provider had already accepted but Orbit could not deliver (no mailbox for the
-- address, another route, a suspended domain or mailbox). No body. Kept for 30 days, for the
-- Failed messages list. No workspace: the recipient's domain is not on this instance.
CREATE TABLE mail_rejected_deliveries (
    id TEXT PRIMARY KEY,
    workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES mail_providers(id) ON DELETE CASCADE,
    mail_from TEXT,
    recipient TEXT NOT NULL CHECK (length(recipient) BETWEEN 1 AND 320),
    reason TEXT NOT NULL CHECK (reason IN ('unknown_address', 'wrong_route', 'domain_unavailable',
        'mailbox_suspended')),
    received_at INTEGER NOT NULL
);

CREATE INDEX mail_rejected_deliveries_workspace ON mail_rejected_deliveries (workspace_id, received_at);
CREATE INDEX mail_rejected_deliveries_received ON mail_rejected_deliveries (received_at);

-- Test addresses of the Admin health card (`check-…@<domain>`, valid for one hour). Mail to one
-- records what arrived and never reaches a mailbox.
CREATE TABLE mail_health_tests (
    id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL REFERENCES mail_domains(id) ON DELETE CASCADE,
    address_local TEXT NOT NULL CHECK (length(address_local) BETWEEN 1 AND 64),
    expires_at INTEGER NOT NULL,
    -- Peer, TLS and SPF/DKIM/DMARC of the test message, once it arrived.
    result TEXT CHECK (result IS NULL OR json_valid(result)),
    received_at INTEGER,
    created_at INTEGER NOT NULL,
    UNIQUE (domain_id, address_local)
);

-- "Always load images from this sender", for each member.
CREATE TABLE mail_sender_prefs (
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    -- The normalized sender address.
    address TEXT NOT NULL CHECK (length(address) BETWEEN 3 AND 320),
    load_images INTEGER NOT NULL CHECK (load_images IN (0, 1)),
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (membership_id, address)
) WITHOUT ROWID;
