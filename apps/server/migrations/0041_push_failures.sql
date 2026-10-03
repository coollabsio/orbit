-- How many pushes to a subscription failed in a row. A push service that refuses the
-- subscription again and again (not "gone", which removes it at once) will not take it later
-- either: the server removes the subscription after a few failures. A push that goes through
-- sets the count back to zero.
ALTER TABLE push_subscriptions ADD COLUMN failures INTEGER NOT NULL DEFAULT 0 CHECK (failures >= 0);
