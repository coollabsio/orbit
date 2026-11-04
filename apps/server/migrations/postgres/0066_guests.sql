-- Guests: outside people who only see the private chat channels they are invited to (see the
-- SQLite 0066, which must rebuild both tables to change their CHECK).
ALTER TABLE workspace_invitations DROP CONSTRAINT workspace_invitations_role_check;
ALTER TABLE workspace_invitations ADD CONSTRAINT workspace_invitations_role_check
    CHECK (role IN ('admin', 'member', 'guest'));

ALTER TABLE memberships DROP CONSTRAINT memberships_role_check;
ALTER TABLE memberships ADD CONSTRAINT memberships_role_check
    CHECK (role IN ('owner', 'admin', 'member', 'guest'));

-- A guest never joins #general (guests are only in private channels).
DROP TRIGGER chat_default_channel_for_member ON memberships;
CREATE TRIGGER chat_default_channel_for_member
    AFTER INSERT ON memberships
    FOR EACH ROW
    WHEN (NEW.role <> 'guest')
    EXECUTE FUNCTION chat_default_channel_for_member_fn();

-- The private channels a guest invitation adds the guest to when it is accepted.
CREATE TABLE workspace_invitation_conversations (
    invitation_id TEXT NOT NULL REFERENCES workspace_invitations(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
    PRIMARY KEY (invitation_id, conversation_id)
);

CREATE INDEX workspace_invitation_conversations_conversation
    ON workspace_invitation_conversations (conversation_id);
