-- Setup confirms the owner's email now, but owners set up earlier kept a NULL email_verified_at, so provider
-- sign-in refused to link their account. Setup creates the owner in the same instant it marks the
-- installation initialized.
UPDATE users
SET email_verified_at = created_at
WHERE email_verified_at IS NULL
  AND created_at = (SELECT initialized_at FROM installation_state WHERE id = 1);
