-- Chat: channels (public, private), direct messages, shared categories, messages with threads,
-- reactions, pins, and each member's read state.
--
-- Built to stay fast with hundreds of thousands of messages:
--   * Every list is a range read on one index, ordered by the UUIDv7 message id (no OFFSET).
--   * Unread counts are counters, not count queries: `message_count` on the conversation minus
--     `read_count` on the member. A send writes the conversation row and the sender's row only.
--   * A deleted message leaves no dead row in a list index: it is hard-deleted, except a thread
--     root that still has replies, which stays visible with an empty body.
-- Chat writes are not audited: an audit row makes every open tab refetch the whole workspace.
-- Users are never hard-deleted, so user references need no ON DELETE action and no index.

CREATE TABLE chat_categories (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX chat_categories_workspace ON chat_categories (workspace_id, position);

CREATE TABLE chat_conversations (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('public', 'private', 'dm')),
    -- Normalized by the server (lower case, `-` for spaces); empty for a DM.
    name TEXT NOT NULL DEFAULT '' CHECK (length(name) <= 80),
    topic TEXT NOT NULL DEFAULT '' CHECK (length(topic) <= 250),
    category_id TEXT REFERENCES chat_categories(id) ON DELETE SET NULL,
    -- Order inside its category.
    position INTEGER NOT NULL DEFAULT 0,
    -- `#general`: every workspace member is in it; it cannot be left, archived or made private.
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    -- The sorted member ids of a DM joined by `,`, so one set of people has one DM.
    dm_key TEXT,
    archived_at INTEGER,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_message_at INTEGER,
    -- Live main-list rows of kind 'message'. A member's unread count is this minus `read_count`.
    message_count INTEGER NOT NULL DEFAULT 0 CHECK (message_count >= 0),
    CHECK ((kind = 'dm') = (dm_key IS NOT NULL)),
    CHECK (kind = 'dm' OR length(name) >= 1),
    CHECK (is_default = 0 OR (kind = 'public' AND archived_at IS NULL))
);

CREATE INDEX chat_conversations_workspace ON chat_conversations (workspace_id, kind);
CREATE INDEX chat_conversations_category ON chat_conversations (category_id);
CREATE UNIQUE INDEX chat_conversations_dm ON chat_conversations (workspace_id, dm_key)
    WHERE dm_key IS NOT NULL;
CREATE UNIQUE INDEX chat_conversations_name ON chat_conversations (workspace_id, name)
    WHERE kind <> 'dm' AND archived_at IS NULL;
CREATE UNIQUE INDEX chat_conversations_default ON chat_conversations (workspace_id)
    WHERE is_default = 1;

-- Membership of a conversation and the member's own state in it. The row goes with the
-- workspace membership, so a removed workspace member leaves every conversation.
CREATE TABLE chat_members (
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    notify TEXT NOT NULL CHECK (notify IN ('all', 'mentions', 'muted')),
    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
    -- A position, not a reference: the message it names may be deleted later.
    last_read_message_id TEXT,
    -- How many of the conversation's `message_count` rows are not unread for this member.
    read_count INTEGER NOT NULL DEFAULT 0 CHECK (read_count >= 0),
    -- Unread messages that mention this member (`@user`).
    mention_count INTEGER NOT NULL DEFAULT 0 CHECK (mention_count >= 0),
    -- Unread messages with `@channel` or `@here`. Not shown while the conversation is muted.
    broadcast_count INTEGER NOT NULL DEFAULT 0 CHECK (broadcast_count >= 0),
    joined_at INTEGER NOT NULL,
    PRIMARY KEY (conversation_id, user_id),
    FOREIGN KEY (workspace_id, user_id) REFERENCES memberships(workspace_id, user_id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE INDEX chat_members_user ON chat_members (workspace_id, user_id);

CREATE TRIGGER chat_members_scope_insert
BEFORE INSERT ON chat_members
WHEN NOT EXISTS (
    SELECT 1 FROM chat_conversations
    WHERE chat_conversations.id = NEW.conversation_id
      AND chat_conversations.workspace_id = NEW.workspace_id
)
BEGIN
    SELECT RAISE(ABORT, 'chat member must belong to the conversation''s workspace');
END;

CREATE TABLE chat_messages (
    -- Stable through VACUUM INTO (backups); the search index refers to it.
    row_id INTEGER PRIMARY KEY,
    -- UUIDv7 made by the server; every list and cursor orders by it.
    id TEXT NOT NULL UNIQUE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
    -- Set on a thread reply. A root is never hard-deleted while it has replies.
    thread_root_id TEXT REFERENCES chat_messages(id) ON DELETE CASCADE,
    -- 'pin', 'join' and 'leave' are one-line system rows about `author_id`.
    kind TEXT NOT NULL CHECK (kind IN ('message', 'pin', 'join', 'leave')),
    author_id TEXT NOT NULL REFERENCES users(id),
    -- Markdown source with mention tokens (`<@id>`, `<#id>`, `<!channel>`, `<!here>`).
    body TEXT NOT NULL CHECK (length(body) <= 4000),
    mention_channel INTEGER NOT NULL DEFAULT 0 CHECK (mention_channel IN (0, 1)),
    mention_here INTEGER NOT NULL DEFAULT 0 CHECK (mention_here IN (0, 1)),
    -- A thread reply that also shows in the conversation's main list.
    also_in_channel INTEGER NOT NULL DEFAULT 0 CHECK (also_in_channel IN (0, 1)),
    -- Made by the sender; a send that is tried again finds its first message by it.
    nonce TEXT CHECK (nonce IS NULL OR length(nonce) BETWEEN 1 AND 64),
    pinned_at INTEGER,
    pinned_by TEXT REFERENCES users(id),
    edited_at INTEGER,
    -- Only on a root that still has replies; every other deleted message is removed.
    deleted_at INTEGER,
    created_at INTEGER NOT NULL,
    -- Thread summary, on a root only.
    reply_count INTEGER NOT NULL DEFAULT 0 CHECK (reply_count >= 0),
    last_reply_id TEXT,
    -- The first reply authors, for the avatars of the summary row.
    reply_user_ids TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(reply_user_ids)),
    CHECK (also_in_channel = 0 OR thread_root_id IS NOT NULL),
    CHECK ((pinned_at IS NULL) = (pinned_by IS NULL))
);

-- Every message of a conversation in order: the main list (which skips plain thread replies),
-- the files list, and the conversation's delete cascade.
CREATE INDEX chat_messages_conversation ON chat_messages (conversation_id, id);
CREATE INDEX chat_messages_thread ON chat_messages (thread_root_id, id)
    WHERE thread_root_id IS NOT NULL;
CREATE INDEX chat_messages_pins ON chat_messages (conversation_id, id)
    WHERE pinned_at IS NOT NULL;
CREATE INDEX chat_messages_threads ON chat_messages (conversation_id, last_reply_id)
    WHERE reply_count > 0;
CREATE UNIQUE INDEX chat_messages_nonce ON chat_messages (conversation_id, author_id, nonce)
    WHERE nonce IS NOT NULL;

-- One row for each `<@id>` token whose user is in the conversation. Used to count mentions
-- after a read cursor; the `mentions` field of a message is parsed from its body instead.
CREATE TABLE chat_message_mentions (
    message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    conversation_id TEXT NOT NULL,
    thread_root_id TEXT,
    -- The message shows in the main list (a root, or an "also in channel" reply).
    in_main INTEGER NOT NULL CHECK (in_main IN (0, 1)),
    PRIMARY KEY (message_id, user_id)
) WITHOUT ROWID;

CREATE INDEX chat_message_mentions_user
    ON chat_message_mentions (user_id, conversation_id, message_id);

CREATE TABLE chat_reactions (
    message_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    emoji TEXT NOT NULL CHECK (length(emoji) BETWEEN 1 AND 64),
    user_id TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (message_id, emoji, user_id)
) WITHOUT ROWID;

-- A member's state in one thread. The row exists once the member follows the thread or has
-- opened it. Unread replies are the root's `reply_count` minus `read_reply_count`, and only
-- while `following`.
CREATE TABLE chat_thread_members (
    root_id TEXT NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
    following INTEGER NOT NULL CHECK (following IN (0, 1)),
    last_read_reply_id TEXT,
    read_reply_count INTEGER NOT NULL DEFAULT 0 CHECK (read_reply_count >= 0),
    mention_count INTEGER NOT NULL DEFAULT 0 CHECK (mention_count >= 0),
    PRIMARY KEY (root_id, user_id),
    FOREIGN KEY (workspace_id, user_id) REFERENCES memberships(workspace_id, user_id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE INDEX chat_thread_members_user ON chat_thread_members (workspace_id, user_id, following);
CREATE INDEX chat_thread_members_conversation ON chat_thread_members (conversation_id, user_id);

-- `#general` for every workspace that exists, with every member in it.
INSERT INTO chat_conversations (id, workspace_id, kind, name, is_default, created_by,
                                created_at, updated_at)
SELECT
    substr(seed.ts, 1, 8) || '-' || substr(seed.ts, 9, 4) || '-7' || substr(seed.r, 1, 3) || '-'
        || substr('89ab', 1 + (abs(random()) % 4), 1) || substr(seed.r, 4, 3) || '-'
        || substr(seed.r, 7, 12),
    seed.workspace_id,
    'public',
    'general',
    1,
    seed.created_by,
    seed.now_ms,
    seed.now_ms
FROM (
    SELECT
        workspaces.id AS workspace_id,
        (SELECT memberships.user_id FROM memberships
         WHERE memberships.id = workspaces.owner_membership_id) AS created_by,
        CAST(strftime('%s', 'now') AS INTEGER) * 1000
            + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER) AS now_ms,
        printf('%012x', CAST(strftime('%s', 'now') AS INTEGER) * 1000
            + CAST(substr(strftime('%f', 'now'), 4, 3) AS INTEGER)) AS ts,
        lower(hex(randomblob(9))) AS r
    FROM workspaces
) AS seed
WHERE seed.created_by IS NOT NULL;

INSERT INTO chat_members (conversation_id, user_id, workspace_id, notify, joined_at)
SELECT chat_conversations.id, memberships.user_id, memberships.workspace_id, 'mentions',
       chat_conversations.created_at
FROM chat_conversations
JOIN memberships ON memberships.workspace_id = chat_conversations.workspace_id
WHERE chat_conversations.is_default = 1;

-- From here on a new workspace member joins `#general`; the first member of a workspace
-- (its owner) makes the channel. A trigger covers every place that adds a membership
-- (setup, workspace creation, invitations, imports).
CREATE TRIGGER chat_default_channel_for_member
AFTER INSERT ON memberships
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
