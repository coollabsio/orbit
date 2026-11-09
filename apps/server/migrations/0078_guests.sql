-- Guests: outside people who only see the private chat channels they are invited to.
--
-- SQLite cannot alter a CHECK constraint, so workspace_invitations and memberships are rebuilt
-- (see 0021). The runner holds a transaction with foreign_keys=ON, so PRAGMA foreign_keys cannot
-- change and ALTER TABLE ... RENAME fails on triggers that name the table. defer_foreign_keys
-- moves the RESTRICT/NO ACTION checks to COMMIT; the old rows are copied aside, the table is
-- recreated under the same name and every row is re-inserted.
--
-- DROP TABLE memberships does an implicit DELETE, and three tables reference it ON DELETE
-- CASCADE: task_assignees (0007) and chat_members, chat_thread_members (0037). A cascade is not
-- deferred, so their rows are copied aside first and re-inserted after the new memberships.
-- workspaces.owner_membership_id is DEFERRABLE INITIALLY DEFERRED and is checked at COMMIT.
-- Nothing references workspace_invitations yet.
PRAGMA defer_foreign_keys = ON;

CREATE TABLE workspace_invitations_backup AS SELECT * FROM workspace_invitations;
DROP TABLE workspace_invitations;

CREATE TABLE workspace_invitations (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    normalized_email TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member', 'guest')),
    delivery TEXT NOT NULL CHECK (delivery IN ('manual', 'smtp')),
    token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    invited_by TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    expires_at INTEGER NOT NULL,
    accepted_at INTEGER,
    revoked_at INTEGER,
    replaced_at INTEGER,
    created_at INTEGER NOT NULL
);

INSERT INTO workspace_invitations (id, workspace_id, email, normalized_email, role, delivery,
                                   token_hash, invited_by, expires_at, accepted_at, revoked_at,
                                   replaced_at, created_at)
SELECT id, workspace_id, email, normalized_email, role, delivery,
       token_hash, invited_by, expires_at, accepted_at, revoked_at,
       replaced_at, created_at
FROM workspace_invitations_backup;

DROP TABLE workspace_invitations_backup;

CREATE UNIQUE INDEX workspace_invitations_pending_email
    ON workspace_invitations (workspace_id, normalized_email)
    WHERE accepted_at IS NULL AND revoked_at IS NULL AND replaced_at IS NULL;

CREATE INDEX workspace_invitations_workspace_created
    ON workspace_invitations (workspace_id, created_at, id);

CREATE TABLE memberships_backup AS SELECT * FROM memberships;
CREATE TABLE task_assignees_backup AS SELECT * FROM task_assignees;
CREATE TABLE chat_members_backup AS SELECT * FROM chat_members;
CREATE TABLE chat_thread_members_backup AS SELECT * FROM chat_thread_members;
DROP TABLE memberships;

CREATE TABLE memberships (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'guest')),
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (workspace_id, user_id)
);

-- Before the triggers exist, so no row gets #general or an owner check twice.
INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at)
SELECT id, workspace_id, user_id, role, version, created_at, updated_at
FROM memberships_backup;

INSERT INTO task_assignees (task_id, membership_id, user_id)
SELECT task_id, membership_id, user_id FROM task_assignees_backup;

INSERT INTO chat_members (conversation_id, user_id, workspace_id, notify, favorite,
                          last_read_message_id, read_count, mention_count, broadcast_count,
                          joined_at)
SELECT conversation_id, user_id, workspace_id, notify, favorite,
       last_read_message_id, read_count, mention_count, broadcast_count,
       joined_at
FROM chat_members_backup;

INSERT INTO chat_thread_members (root_id, user_id, workspace_id, conversation_id, following,
                                 last_read_reply_id, read_reply_count, mention_count)
SELECT root_id, user_id, workspace_id, conversation_id, following,
       last_read_reply_id, read_reply_count, mention_count
FROM chat_thread_members_backup;

DROP TABLE memberships_backup;
DROP TABLE task_assignees_backup;
DROP TABLE chat_members_backup;
DROP TABLE chat_thread_members_backup;

CREATE UNIQUE INDEX memberships_one_owner
    ON memberships (workspace_id)
    WHERE role = 'owner';

CREATE INDEX memberships_user_role ON memberships (user_id, role);

CREATE TRIGGER memberships_protect_current_owner_role
BEFORE UPDATE OF role ON memberships
WHEN OLD.role = 'owner' AND NEW.role != 'owner'
 AND EXISTS (
    SELECT 1 FROM workspaces
    WHERE workspaces.id = OLD.workspace_id
      AND workspaces.owner_membership_id = OLD.id
 )
BEGIN
    SELECT RAISE(ABORT, 'workspace owner role is protected');
END;

CREATE TRIGGER memberships_restrict_owner_role
BEFORE UPDATE OF role ON memberships
WHEN NEW.role = 'owner'
 AND NOT EXISTS (
    SELECT 1 FROM workspaces
    WHERE workspaces.id = NEW.workspace_id
      AND workspaces.owner_membership_id = NEW.id
 )
BEGIN
    SELECT RAISE(ABORT, 'workspace owner role must match owner membership');
END;

CREATE TRIGGER memberships_validate_owner_insert
BEFORE INSERT ON memberships
WHEN (
    NEW.role = 'owner' AND NOT EXISTS (
        SELECT 1 FROM workspaces
        WHERE workspaces.id = NEW.workspace_id
          AND workspaces.owner_membership_id = NEW.id
    )
) OR EXISTS (
    SELECT 1 FROM workspaces
    WHERE workspaces.owner_membership_id = NEW.id
      AND (workspaces.id != NEW.workspace_id OR NEW.role != 'owner')
)
BEGIN
    SELECT RAISE(ABORT, 'workspace owner role must match owner membership');
END;

CREATE TRIGGER memberships_identity_immutable
BEFORE UPDATE OF workspace_id, user_id ON memberships
WHEN NEW.workspace_id <> OLD.workspace_id OR NEW.user_id <> OLD.user_id
BEGIN
    SELECT RAISE(ABORT, 'membership identity is immutable');
END;

-- As in 0037, except that a guest never joins #general (guests are only in private channels).
CREATE TRIGGER chat_default_channel_for_member
AFTER INSERT ON memberships
WHEN NEW.role <> 'guest'
BEGIN
    INSERT INTO chat_conversations (id, workspace_id, kind, name, is_default, created_by,
                                    created_at, updated_at)
    SELECT
        substr(seed.ts, 1, 8) || '-' || substr(seed.ts, 9, 4) || '-7' || substr(seed.r, 1, 3) || '-'
            || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(seed.r, 4, 3) || '-'
            || substr(seed.r, 7, 12),
        NEW.workspace_id,
        'public',
        'general',
        1,
        NEW.user_id,
        NEW.created_at,
        NEW.created_at
    FROM (SELECT printf('%012x', NEW.created_at) AS ts, lower(hex(randomblob(9))) AS r) AS seed
    WHERE NOT EXISTS (
        SELECT 1 FROM chat_conversations
        WHERE chat_conversations.workspace_id = NEW.workspace_id
          AND chat_conversations.is_default = 1
    );

    -- A member who joins starts with everything read.
    INSERT INTO chat_members (conversation_id, user_id, workspace_id, notify,
                              last_read_message_id, read_count, joined_at)
    SELECT chat_conversations.id, NEW.user_id, NEW.workspace_id, 'mentions',
           (SELECT chat_messages.id FROM chat_messages
            WHERE chat_messages.conversation_id = chat_conversations.id
            ORDER BY chat_messages.id DESC LIMIT 1),
           chat_conversations.message_count,
           NEW.created_at
    FROM chat_conversations
    WHERE chat_conversations.workspace_id = NEW.workspace_id
      AND chat_conversations.is_default = 1;
END;

-- The private channels a guest invitation adds the guest to when it is accepted.
CREATE TABLE workspace_invitation_conversations (
    invitation_id TEXT NOT NULL REFERENCES workspace_invitations(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
    PRIMARY KEY (invitation_id, conversation_id)
) WITHOUT ROWID;

CREATE INDEX workspace_invitation_conversations_conversation
    ON workspace_invitation_conversations (conversation_id);
