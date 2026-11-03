import { Link } from 'react-router'
import { useAuthOptions, useCurrentUser } from '@/features/auth/api'

/**
 * Orbit Cloud with no mail server (only right after setup): a line above the top bar for installation admins until
 * they save one. Customers see nothing.
 */
export function MailServerBanner() {
  const options = useAuthOptions().data
  const admin = useCurrentUser().data?.installation_admin ?? false
  if (!admin || !options?.billing_enabled || options.email_enabled) return null
  return (
    <div
      data-slot="mail-server-banner"
      role="alert"
      className="flex min-h-8 shrink-0 flex-wrap items-center justify-center gap-x-2 border-b border-destructive/20 bg-destructive/10 px-3 py-1 text-center text-xs text-destructive dark:bg-destructive/20"
    >
      <span className="font-medium">
        Orbit Cloud has no mail server. Email change, invitations by email, password reset and sign-up do not work.
      </span>
      <Link to="/admin/email" className="shrink-0 font-semibold text-destructive underline underline-offset-4">
        Set it in Admin → Email
      </Link>
    </div>
  )
}
