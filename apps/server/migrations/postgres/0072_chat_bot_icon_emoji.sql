-- The emoji avatar of an integration's message (Slack's `icon_emoji`), as an emoji name without colons.
ALTER TABLE chat_messages ADD COLUMN bot_icon_emoji TEXT;
