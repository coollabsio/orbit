-- Postgres: the same change as apps/server/migrations/20261125083424_mail_send_unlimited_tier.sql
ALTER TABLE mail_send_policies DROP CONSTRAINT mail_send_policies_tier_check;
ALTER TABLE mail_send_policies ADD CONSTRAINT mail_send_policies_tier_check
    CHECK (tier IN ('new', 'standard', 'custom', 'unlimited'));
