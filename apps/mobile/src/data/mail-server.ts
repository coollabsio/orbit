import type { AuthOptions } from '@orbit/api/generated/types.gen'

export const MAIL_SERVER_WARNING = 'Orbit Cloud has no mail server. Email change, invitations by email, password reset and sign-up do not work.'

/** The web's `MailServerBanner`: Orbit Cloud without a mail server warns its installation admins, never customers. */
export function mailServerMissing(options: Pick<AuthOptions, 'billing_enabled' | 'email_enabled'> | undefined, installationAdmin: boolean): boolean {
  return installationAdmin && options?.billing_enabled === true && !options.email_enabled
}
