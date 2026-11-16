-- Mail, slice 2 (send): send routes, drafts, the outbound queue with its recipients, suppressions,
-- signatures, snippets, vacation replies, contacts and file links.
--
-- * Times are milliseconds since the epoch; flags are 0/1 INTEGER.
-- * `mail_draft_files.blob_id` and `mail_file_links.blob_id` are blob references like
--   `mail_receipts.blob_id`, so the blob reconcile keeps a blob while one of them uses it.

-- A way to send mail out. No workspace: an instance route of the root user, which every workspace
-- may pick; else a route that only its workspace may pick. Kind `direct` sends from Orbit itself.
CREATE TABLE mail_send_routes (
    id TEXT PRIMARY KEY,
    workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('direct', 'smtp', 'postshiba', 'postmark', 'resend', 'mailgun',
        'ses', 'cloudflare')),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    -- The receiving provider this route belongs to, when there is one.
    provider_id TEXT REFERENCES mail_providers(id) ON DELETE SET NULL,
    -- JSON of the kind's secrets, sealed with the app key; associated data `mail-send-route:<id>`.
    secrets BLOB NOT NULL,
    -- Host, port, security, region, base URL, account ID, stream and max bytes (JSON).
    settings TEXT NOT NULL CHECK (json_valid(settings)),
    last_sent_at INTEGER,
    last_error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX mail_send_routes_workspace ON mail_send_routes (workspace_id);
CREATE INDEX mail_send_routes_provider ON mail_send_routes (provider_id) WHERE provider_id IS NOT NULL;

-- The route a domain sends through; NULL: the instance default. A route in use cannot be deleted
-- (NO ACTION, so a workspace's cascade still removes both).
ALTER TABLE mail_domains ADD COLUMN send_route_id TEXT REFERENCES mail_send_routes(id);

CREATE INDEX mail_domains_send_route ON mail_domains (send_route_id) WHERE send_route_id IS NOT NULL;

-- A domain may use an instance route or a route of its own workspace.
CREATE TRIGGER mail_domains_send_route_scope_insert
BEFORE INSERT ON mail_domains
WHEN NEW.send_route_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM mail_send_routes
    WHERE id = NEW.send_route_id AND (workspace_id IS NULL OR workspace_id = NEW.workspace_id))
BEGIN SELECT RAISE(ABORT, 'mail domain must use a send route of its workspace'); END;

CREATE TRIGGER mail_domains_send_route_scope_update
BEFORE UPDATE OF send_route_id, workspace_id ON mail_domains
WHEN NEW.send_route_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM mail_send_routes
    WHERE id = NEW.send_route_id AND (workspace_id IS NULL OR workspace_id = NEW.workspace_id))
BEGIN SELECT RAISE(ABORT, 'mail domain must use a send route of its workspace'); END;

-- The instance default route and the limits of the `new` and `standard` tiers (JSON).
ALTER TABLE instance_settings ADD COLUMN mail_send_route_id TEXT REFERENCES mail_send_routes(id)
    ON DELETE SET NULL;
ALTER TABLE instance_settings ADD COLUMN mail_send_tiers TEXT NOT NULL
    DEFAULT '{"new":{"per_user_hour":30,"per_user_day":100,"per_ws_hour":60,"per_ws_day":200,"max_recipients":50},"standard":{"per_user_hour":500,"per_user_day":2000,"per_ws_hour":2000,"per_ws_day":10000,"max_recipients":100}}'
    CHECK (json_valid(mail_send_tiers));

CREATE TABLE mail_send_policies (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    tier TEXT NOT NULL CHECK (tier IN ('new', 'standard', 'custom')),
    custom_limits TEXT CHECK (custom_limits IS NULL OR json_valid(custom_limits)),
    -- Days a file link of an outgoing message stays valid.
    link_days INTEGER NOT NULL DEFAULT 30 CHECK (link_days BETWEEN 1 AND 365),
    first_release_at INTEGER,
    paused_at INTEGER,
    pause_reason TEXT,
    resumed_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE mail_drafts (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    from_address_id TEXT NOT NULL REFERENCES mail_addresses(id) ON DELETE CASCADE,
    thread_id TEXT REFERENCES mail_threads(id) ON DELETE CASCADE,
    parent_message_id TEXT REFERENCES mail_messages(id) ON DELETE SET NULL,
    mode TEXT NOT NULL CHECK (mode IN ('new', 'reply', 'reply_all', 'forward', 'forward_attached')),
    to_addrs TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(to_addrs)),
    cc_addrs TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(cc_addrs)),
    bcc_addrs TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(bcc_addrs)),
    subject TEXT NOT NULL DEFAULT '',
    body_html TEXT NOT NULL DEFAULT '',
    version INTEGER NOT NULL DEFAULT 1 CHECK (version >= 1),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    -- One reply draft for each member and thread.
    UNIQUE (thread_id, membership_id)
);

CREATE INDEX mail_drafts_member ON mail_drafts (membership_id, updated_at);
CREATE INDEX mail_drafts_mailbox ON mail_drafts (mailbox_id);
CREATE INDEX mail_drafts_workspace ON mail_drafts (workspace_id);

CREATE TABLE mail_draft_files (
    id TEXT PRIMARY KEY,
    draft_id TEXT NOT NULL REFERENCES mail_drafts(id) ON DELETE CASCADE,
    -- For the workspace purge, which removes blob references before the blobs.
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    blob_id TEXT NOT NULL REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    filename TEXT NOT NULL CHECK (length(filename) BETWEEN 1 AND 255),
    content_type TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size >= 0),
    -- The Content-ID of an inline image.
    content_id TEXT,
    -- Sent as a download link instead of an attachment.
    as_link INTEGER NOT NULL DEFAULT 0 CHECK (as_link IN (0, 1)),
    -- The received attachment this one was forwarded from.
    source_attachment_id TEXT REFERENCES mail_attachments(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX mail_draft_files_draft ON mail_draft_files (draft_id);
CREATE INDEX mail_draft_files_blob ON mail_draft_files (blob_id);
CREATE INDEX mail_draft_files_workspace ON mail_draft_files (workspace_id);

-- A message to send: held for the undo time or until its scheduled time, then released.
CREATE TABLE mail_outbound (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    -- NULL for a send without a draft (a vacation reply). Unique: a draft is sent once.
    draft_id TEXT UNIQUE REFERENCES mail_drafts(id) ON DELETE SET NULL,
    -- The sent message, once released.
    message_id TEXT REFERENCES mail_messages(id) ON DELETE SET NULL,
    route_id TEXT REFERENCES mail_send_routes(id) ON DELETE SET NULL,
    status TEXT NOT NULL CHECK (status IN ('held', 'cancelled', 'released', 'done')),
    release_at INTEGER NOT NULL,
    scheduled INTEGER NOT NULL DEFAULT 0 CHECK (scheduled IN (0, 1)),
    hold_reason TEXT,
    -- The final status of each recipient (JSON), kept after the recipient rows are purged.
    delivery_summary TEXT CHECK (delivery_summary IS NULL OR json_valid(delivery_summary)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX mail_outbound_due ON mail_outbound (status, release_at);
CREATE INDEX mail_outbound_workspace ON mail_outbound (workspace_id);
CREATE INDEX mail_outbound_membership ON mail_outbound (membership_id);
CREATE INDEX mail_outbound_mailbox ON mail_outbound (mailbox_id);
CREATE INDEX mail_outbound_message ON mail_outbound (message_id) WHERE message_id IS NOT NULL;
CREATE INDEX mail_outbound_route ON mail_outbound (route_id) WHERE route_id IS NOT NULL;

CREATE TABLE mail_outbound_recipients (
    id TEXT PRIMARY KEY,
    outbound_id TEXT NOT NULL REFERENCES mail_outbound(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    address TEXT NOT NULL CHECK (length(address) BETWEEN 3 AND 320),
    kind TEXT NOT NULL CHECK (kind IN ('to', 'cc', 'bcc')),
    status TEXT NOT NULL CHECK (status IN ('queued', 'sending', 'delivered', 'deferred', 'bounced',
        'suppressed', 'cancelled')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    -- Set while one worker sends to this recipient.
    attempt_id TEXT,
    next_attempt_at INTEGER,
    last_response TEXT,
    provider_message_id TEXT,
    -- Why a bounced recipient bounced: `hard` (a 5xx refusal of the recipient), `gave_up` (still
    -- deferred after the last retry), `route` (gave up on route errors), `unreadable` (the sent
    -- message could not be read). Only `hard` counts toward the bounce rate and suppressions.
    bounce_kind TEXT CHECK (bounce_kind IS NULL OR bounce_kind IN ('hard', 'gave_up', 'route',
        'unreadable')),
    -- When the send counted against the limits.
    released_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX mail_outbound_recipients_outbound ON mail_outbound_recipients (outbound_id);
CREATE INDEX mail_outbound_recipients_due ON mail_outbound_recipients (status, next_attempt_at);
CREATE INDEX mail_outbound_recipients_workspace_limit ON mail_outbound_recipients (workspace_id, released_at);
CREATE INDEX mail_outbound_recipients_member_limit ON mail_outbound_recipients (membership_id, released_at);
CREATE INDEX mail_outbound_recipients_provider ON mail_outbound_recipients (provider_message_id)
    WHERE provider_message_id IS NOT NULL;

-- Addresses that bounced hard, complained or were set by an admin: nothing is sent to them.
CREATE TABLE mail_suppressions (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    -- Lower case.
    address TEXT NOT NULL CHECK (length(address) BETWEEN 3 AND 320),
    reason TEXT NOT NULL CHECK (reason IN ('hard_bounce', 'complaint', 'manual')),
    detail TEXT,
    recipient_id TEXT REFERENCES mail_outbound_recipients(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, address)
) WITHOUT ROWID;

CREATE TABLE mail_signatures (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    address_id TEXT NOT NULL REFERENCES mail_addresses(id) ON DELETE CASCADE,
    body_html TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (membership_id, address_id)
);

CREATE INDEX mail_signatures_address ON mail_signatures (address_id);
CREATE INDEX mail_signatures_workspace ON mail_signatures (workspace_id);

-- Personal text snippets.
CREATE TABLE mail_snippets (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    body_html TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX mail_snippets_member ON mail_snippets (membership_id);
CREATE INDEX mail_snippets_workspace ON mail_snippets (workspace_id);

CREATE TABLE mail_member_settings (
    membership_id TEXT PRIMARY KEY REFERENCES memberships(id) ON DELETE CASCADE,
    undo_send_seconds INTEGER NOT NULL DEFAULT 10 CHECK (undo_send_seconds IN (0, 5, 10, 20, 30)),
    updated_at INTEGER NOT NULL
);

CREATE TABLE mail_vacations (
    mailbox_id TEXT PRIMARY KEY REFERENCES mailboxes(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    starts_at INTEGER,
    ends_at INTEGER,
    subject TEXT NOT NULL DEFAULT '',
    body_html TEXT NOT NULL DEFAULT '',
    contacts_only INTEGER NOT NULL DEFAULT 0 CHECK (contacts_only IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Who got a vacation reply and when: one reply to a sender in 7 days.
CREATE TABLE mail_vacation_replies (
    mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    sender TEXT NOT NULL CHECK (length(sender) BETWEEN 3 AND 320),
    replied_at INTEGER NOT NULL,
    PRIMARY KEY (mailbox_id, sender)
) WITHOUT ROWID;

CREATE INDEX mail_vacation_replies_replied ON mail_vacation_replies (replied_at);

-- The people a member wrote to, for the address picker.
CREATE TABLE mail_contacts (
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    membership_id TEXT NOT NULL REFERENCES memberships(id) ON DELETE CASCADE,
    address TEXT NOT NULL CHECK (length(address) BETWEEN 3 AND 320),
    name TEXT,
    send_count INTEGER NOT NULL DEFAULT 0 CHECK (send_count >= 0),
    last_sent_at INTEGER,
    hidden INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (membership_id, address)
) WITHOUT ROWID;

CREATE INDEX mail_contacts_workspace ON mail_contacts (workspace_id);

-- A file sent as a download link. The token is stored as a SHA-256 hash (hex).
CREATE TABLE mail_file_links (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    outbound_id TEXT NOT NULL REFERENCES mail_outbound(id) ON DELETE CASCADE,
    blob_id TEXT NOT NULL REFERENCES attachment_blobs(id) ON DELETE RESTRICT,
    filename TEXT NOT NULL CHECK (length(filename) BETWEEN 1 AND 255),
    size INTEGER NOT NULL CHECK (size >= 0),
    token_sha256 TEXT NOT NULL CHECK (length(token_sha256) = 64),
    expires_at INTEGER NOT NULL,
    revoked_at INTEGER,
    downloads INTEGER NOT NULL DEFAULT 0 CHECK (downloads >= 0),
    created_at INTEGER NOT NULL,
    UNIQUE (token_sha256)
);

CREATE INDEX mail_file_links_outbound ON mail_file_links (outbound_id);
CREATE INDEX mail_file_links_blob ON mail_file_links (blob_id);
CREATE INDEX mail_file_links_workspace ON mail_file_links (workspace_id);
CREATE INDEX mail_file_links_expires ON mail_file_links (expires_at);

-- 1 for a message sent from the mailbox, 0 for a received one.
ALTER TABLE mail_thread_messages ADD COLUMN outgoing INTEGER NOT NULL DEFAULT 0
    CHECK (outgoing IN (0, 1));

ALTER TABLE mail_threads ADD COLUMN last_sent_at INTEGER;
ALTER TABLE mail_threads ADD COLUMN last_incoming_at INTEGER;
UPDATE mail_threads SET last_incoming_at = last_message_at;
CREATE INDEX mail_threads_sent ON mail_threads (mailbox_id, last_sent_at);
