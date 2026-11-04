import { toast } from 'sonner'
import type { MailProviderKind, MailProviderView } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { fullDate, relativeTime, timeOfDay } from '@/lib/format'

/** The server's words for a refused mail request (its `detail` names the reason), else `fallback`. */
export function mailProblem(error: unknown, fallback: string): string {
  return error instanceof ApiProblem ? error.detail : `${fallback} Orbit did not answer. Check your connection and try again.`
}

/** The instance admin has not let workspaces add mail domains (Admin → Mail). */
export const isDomainsDisabled = (error: unknown) => error instanceof ApiProblem && error.code === 'mail_domains_disabled'

/** Copies `text` and says so; `what` names it in the toast ("Name copied"). */
export function copyText(text: string, what: string) {
  void navigator.clipboard.writeText(text).then(
    () => toast(`${what} copied`),
    () => toast.error('Could not copy to the clipboard.'),
  )
}

/** "Fri, Oct 23, 2026, 14:05". */
export const dateTime = (iso: string) => `${fullDate(iso)}, ${timeOfDay(iso)}`

/** "just now", "5m ago", or "on Oct 3" for a week or more. */
export function since(iso: string): string {
  const ago = relativeTime(iso)
  if (ago === 'now') return 'just now'
  return /^\d+[mhd]$/.test(ago) ? `${ago} ago` : `on ${ago}`
}

/** `1 address`, `2 addresses`. */
export const count = (amount: number, one: string, many: string) => `${amount} ${amount === 1 ? one : many}`

export const PROVIDER_LABELS: Record<MailProviderKind, string> = {
  cloudflare: 'Cloudflare Email Routing',
  postmark: 'Postmark',
  mailgun: 'Mailgun',
  resend: 'Resend',
  ses: 'Amazon SES',
}

/** The route value for this server's own SMTP listener. */
export const DIRECT = 'direct'

/** "Directly (MX to this server)", then each provider the workspace may use. */
export function routeOptions(providers: MailProviderView[]) {
  return [
    { value: DIRECT, label: 'Directly (MX to this server)' },
    ...providers.map((provider) => ({ value: provider.id, label: `${provider.name} (${PROVIDER_LABELS[provider.kind]})` })),
  ]
}
