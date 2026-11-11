import { toast } from 'sonner'
import type { MailDnsRecordKind, MailDomain, MailProviderKind, MailProviderView } from '@orbit/api/generated/types.gen'
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

export const KIND_LABELS: Record<MailDnsRecordKind, string> = {
  verify: 'Verification',
  mx: 'Mail server',
  dkim: 'DKIM',
  spf: 'SPF',
  dmarc: 'DMARC',
}

/** Mail reaches the workspace only with these; DKIM, SPF and DMARC are for sending (a later release) and reputation. */
export const REQUIRED: ReadonlySet<MailDnsRecordKind> = new Set(['verify', 'mx'])

export interface DomainHealth {
  label: string
  variant: 'secondary' | 'outline' | 'destructive'
  hint: string
}

/** One badge for a domain: its status, and for a verified domain whether its DNS records are all in place. */
export function domainHealth(domain: Pick<MailDomain, 'status' | 'records'>): DomainHealth {
  switch (domain.status) {
    case 'pending':
      return { label: 'Pending', variant: 'outline', hint: 'Publish the verification record, then click Check now.' }
    case 'suspended':
      return { label: 'Suspended', variant: 'destructive', hint: 'The verification record is gone. Mail is accepted only during the grace period.' }
    case 'disabled':
      return { label: 'Disabled', variant: 'outline', hint: 'Mail for this domain is refused.' }
    case 'verified': {
      const bad = domain.records.filter((record) => record.status.status === 'missing' || record.status.status === 'wrong')
      const names = (required: boolean) => bad.filter((record) => REQUIRED.has(record.kind) === required).map((record) => KIND_LABELS[record.kind])
      const required = names(true)
      const recommended = names(false)
      if (required.length > 0) return { label: 'Action needed', variant: 'destructive', hint: `Missing or wrong: ${required.join(', ')}. Mail cannot arrive without it.` }
      if (recommended.length > 0) return { label: 'Incomplete', variant: 'outline', hint: `Verified. Recommended records missing or wrong: ${recommended.join(', ')}.` }
      return { label: 'Healthy', variant: 'secondary', hint: 'Verified, and all DNS records are in place.' }
    }
  }
}
