// The composer's rules, without React: pre-send checks (§9.1 U5), message size and file links (§5.9), the
// recipient line and fields, send-later times, and where the signature sits in the text.

export type Recipient = string | { address: string; name?: string | null }

const addressOf = (recipient: Recipient) => (typeof recipient === 'string' ? recipient : recipient.address)
const lower = (address: string) => address.trim().toLowerCase()
const domainOf = (address: string) => lower(address).split('@').at(-1) ?? ''

// Pre-send checks

export type WarningKind = 'missing_attachment' | 'empty_subject' | 'placeholder' | 'new_outside_recipient' | 'reply_all_over_10' | 'suppressed'

export interface Warning {
  kind: WarningKind
  message: string
  /** The recipients it is about (outside or suppressed ones). */
  addresses?: string[]
}

export interface PreSendInput {
  /** The text the user wrote (plain). */
  text: string
  subject: string
  fileCount: number
  /** `new`, `reply`, `reply_all`, `forward` or `forward_attached`. */
  mode: string
  to: readonly string[]
  cc: readonly string[]
  bcc: readonly string[]
  /** Everyone on the thread so far (senders and recipients); `null` for a new message. */
  threadAddresses: readonly string[] | null
  /** The workspace's mail domains. */
  workspaceDomains: readonly string[]
  /** Recipients known to be on the suppression list (lower case). */
  suppressed: Iterable<string>
}

const ATTACHMENT_WORD = /\battach(ed|ment|ments)?\b/i
const PLACEHOLDER = /\{[^}]+\}/
/** Reply all to more people than this asks first. */
export const REPLY_ALL_LIMIT = 10

/** Distinct recipients in the order they were entered (the first spelling of each address). */
function distinctRecipients(input: Pick<PreSendInput, 'to' | 'cc' | 'bcc'>): string[] {
  const seen = new Set<string>()
  return [...input.to, ...input.cc, ...input.bcc].filter((address) => {
    const key = lower(address)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

const people = (addresses: readonly string[]) => (addresses.length === 1 ? addresses[0] : `${addresses.length} people`)

/**
 * Checks shown above Send (not a dialog); sending again sends anyway. In the order: a missing attachment, an empty
 * subject, a placeholder left from a snippet, a new outside recipient in a workspace-only thread, reply all to more
 * than 10 people, a suppressed recipient.
 */
export function preSendWarnings(input: PreSendInput): Warning[] {
  const warnings: Warning[] = []
  const recipients = distinctRecipients(input)
  if (ATTACHMENT_WORD.test(input.text) && input.fileCount === 0 && input.mode !== 'forward_attached') {
    warnings.push({ kind: 'missing_attachment', message: 'You mention an attachment, but no file is attached.' })
  }
  if (input.subject.trim() === '') warnings.push({ kind: 'empty_subject', message: 'The subject is empty.' })
  const placeholder = PLACEHOLDER.exec(input.text)?.[0] ?? PLACEHOLDER.exec(input.subject)?.[0]
  if (placeholder) warnings.push({ kind: 'placeholder', message: `${placeholder} is still a placeholder.` })

  if (input.threadAddresses && input.threadAddresses.length > 0) {
    const domains = new Set(input.workspaceDomains.map((domain) => domain.toLowerCase()))
    const inside = (address: string) => domains.has(domainOf(address))
    const onThread = new Set(input.threadAddresses.map(lower))
    if (input.threadAddresses.every(inside)) {
      const outside = recipients.filter((address) => !inside(address) && !onThread.has(lower(address)))
      if (outside.length > 0) {
        warnings.push({
          kind: 'new_outside_recipient',
          message: `${people(outside)} ${outside.length === 1 ? 'is' : 'are'} outside the workspace. This thread had only workspace addresses.`,
          addresses: outside,
        })
      }
    }
  }
  if (input.mode === 'reply_all' && recipients.length > REPLY_ALL_LIMIT) {
    warnings.push({ kind: 'reply_all_over_10', message: `Reply all goes to ${recipients.length} people.` })
  }
  const suppressedSet = new Set([...input.suppressed].map(lower))
  const suppressed = recipients.filter((address) => suppressedSet.has(lower(address)))
  if (suppressed.length > 0) {
    warnings.push({
      kind: 'suppressed',
      message: `Mail to ${people(suppressed)} bounced before. Orbit will skip ${suppressed.length === 1 ? 'it' : 'them'}.`,
      addresses: suppressed,
    })
  }
  return warnings
}

// Size and file links

/** Line breaks in base64 (every 76 characters) add about 2 %. */
const LINE_OVERHEAD = 1.02

/** The size a file adds to the message once encoded: base64 (4 bytes for each 3) plus line breaks. */
const encodedFileSize = (size: number) => Math.ceil(4 * Math.ceil(size / 3) * LINE_OVERHEAD)

/** The encoded size of a message: the body as it is plus every file that is not sent as a link, in base64. */
export function encodedSize(files: readonly { size: number; asLink: boolean }[], bodyBytes: number): number {
  return files.reduce((total, file) => (file.asLink ? total : total + encodedFileSize(file.size)), bodyBytes)
}

/** Headers and the MIME structure, as the server counts them. */
export const MESSAGE_OVERHEAD_BYTES = 4096

export interface SizedFile {
  id: string
  size: number
  asLink: boolean
  /** Inline images and forwarded files must travel with the message. */
  linkable: boolean
}

/** The files to send as links, largest first, until the message fits under `limit` (all of them when it never does). */
export function suggestLinks(files: readonly SizedFile[], limit: number, bodyBytes = 0): string[] {
  let size = encodedSize(files, bodyBytes)
  const picked: string[] = []
  const candidates = files.filter((file) => file.linkable && !file.asLink).sort((a, b) => b.size - a.size)
  for (const file of candidates) {
    if (size <= limit) break
    picked.push(file.id)
    size -= encodedFileSize(file.size)
  }
  return picked
}

/**
 * Why a draft file cannot be sent as a link (the server answers `link_forwarded` / `link_inline`), or `null` when it
 * can: a forwarded file and an image in the text must travel with the message.
 */
export function linkRefusal(file: { source_attachment_id?: string | null; content_id?: string | null }): string | null {
  if (file.source_attachment_id) return 'A forwarded file travels with the message.'
  if (file.content_id?.trim()) return 'An image in the text travels with the message.'
  return null
}

/** The size limits of the send routes (bytes, encoded): Postmark, direct/SMTP/Mailgun/PostShiba, Cloudflare, SES. */
export const ROUTE_LIMITS = [10_000_000, 25_000_000, 26_214_400, 40_000_000] as const

/**
 * The server says only that a message is too large for its route. The limit it was over is most likely the largest
 * route limit below the message's size; linking files to fit under it, then sending again, converges.
 */
export function likelyLimit(size: number): number {
  return ROUTE_LIMITS.filter((limit) => limit < size).at(-1) ?? ROUTE_LIMITS[0]
}

// Recipients

/** The one-line recipients of an inline reply: "To Andras +2". */
export function recipientSummary(to: readonly Recipient[], cc: readonly Recipient[], bcc: readonly Recipient[]): string {
  const [label, list] = to.length > 0 ? ['To', to] : cc.length > 0 ? ['Cc', cc] : bcc.length > 0 ? ['Bcc', bcc] : [null, []]
  const first = list[0]
  if (!label || first === undefined) return 'No recipients'
  const name = typeof first === 'string' ? '' : (first.name?.trim().split(/\s+/)[0] ?? '')
  const others = to.length + cc.length + bcc.length - 1
  return `${label} ${name || addressOf(first)}${others > 0 ? ` +${others}` : ''}`
}

/**
 * Typed or pasted text as recipients: commas, semicolons and line breaks separate them (not inside quotes or
 * brackets), `Name <address>` keeps the address, and spaces separate bare addresses. Text without an address stays
 * as one entry (shown as invalid).
 */
export function splitRecipients(text: string): string[] {
  const pieces: string[] = []
  let current = ''
  let quoted = false
  let bracketed = false
  for (const char of text) {
    if (char === '"') quoted = !quoted
    else if (char === '<') bracketed = true
    else if (char === '>') bracketed = false
    if (!quoted && !bracketed && /[,;\n\r\t]/.test(char)) {
      pieces.push(current)
      current = ''
    } else {
      current += char
    }
  }
  pieces.push(current)
  return pieces.flatMap((piece) => {
    const bracket = /<([^>]*)>/.exec(piece)
    if (bracket) return [bracket[1]!.trim()].filter(Boolean)
    const words = piece.trim().split(/\s+/).filter(Boolean)
    const addresses = words.filter((word) => word.includes('@'))
    if (addresses.length > 0) return addresses
    const rest = piece.trim().replace(/^"|"$/g, '').trim()
    return rest ? [rest] : []
  })
}

const LOCAL_PART = /^[A-Za-z0-9.!#$&'*+/=?^_`{|}~-]+$/
const DOMAIN = /^(?=.{1,253}$)([A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\.)+[A-Za-z]{2,63}$/

/** An address the server accepts: no name, quotes, routing (`%`, `!`) or spaces; a domain with a dot. */
export function isValidAddress(address: string): boolean {
  if (address.length > 254) return false
  const parts = address.split('@')
  if (parts.length !== 2) return false
  const [local, domain] = parts as [string, string]
  return local.length > 0 && local.length <= 64 && LOCAL_PART.test(local) && !/[%!]/.test(local) && DOMAIN.test(domain)
}

// Send later

interface WallClock {
  year: number
  month: number
  day: number
  weekday: number
}

/** The calendar date and weekday (0 = Sunday) of `at` in `timeZone`. */
function wallClock(at: Date, timeZone: string): WallClock {
  const parts = new Intl.DateTimeFormat('en-US', { timeZone, year: 'numeric', month: 'numeric', day: 'numeric', weekday: 'short' }).formatToParts(at)
  const part = (type: string) => parts.find((entry) => entry.type === type)?.value ?? ''
  const weekday = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'].indexOf(part('weekday'))
  return { year: Number(part('year')), month: Number(part('month')), day: Number(part('day')), weekday }
}

/** How far `timeZone` is ahead of UTC at `at`, in ms. */
function zoneOffset(at: Date, timeZone: string): number {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone,
    hourCycle: 'h23',
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: 'numeric',
    minute: 'numeric',
    second: 'numeric',
  }).formatToParts(at)
  const part = (type: string) => Number(parts.find((entry) => entry.type === type)?.value ?? 0)
  const asUtc = Date.UTC(part('year'), part('month') - 1, part('day'), part('hour'), part('minute'), part('second'))
  return asUtc - Math.floor(at.getTime() / 1000) * 1000
}

/** The instant a wall-clock time is in `timeZone` (days past the month's end roll over). */
function zonedTime(year: number, month: number, day: number, hour: number, timeZone: string): Date {
  const guess = Date.UTC(year, month - 1, day, hour)
  const first = guess - zoneOffset(new Date(guess), timeZone)
  // the offset can differ at the answer (a daylight saving change in between)
  return new Date(guess - zoneOffset(new Date(first), timeZone))
}

export const SEND_LATER_HOUR = 8

/** **Send later** choices in the user's time zone: tomorrow at 8:00 and the next Monday at 8:00 (in 7 days on a Monday). */
export function sendLaterPresets(now: Date, timeZone: string): { label: string; at: Date }[] {
  const today = wallClock(now, timeZone)
  const toMonday = ((8 - today.weekday) % 7) || 7
  return [
    { label: `Tomorrow ${SEND_LATER_HOUR}:00`, at: zonedTime(today.year, today.month, today.day + 1, SEND_LATER_HOUR, timeZone) },
    { label: `Monday ${SEND_LATER_HOUR}:00`, at: zonedTime(today.year, today.month, today.day + toMonday, SEND_LATER_HOUR, timeZone) },
  ]
}

// Signature

const normalized = (text: string) => text.replace(/\s+/g, ' ').trim()

/**
 * Where the signature starts in the text's lines (one per block), or `null` when the text does not end with it (the
 * user changed or removed it). Empty lines are ignored at the end and inside the signature.
 */
export function signatureStart(lines: readonly string[], signature: readonly string[]): number | null {
  const wanted = signature.map(normalized).filter(Boolean)
  if (wanted.length === 0) return null
  let index = lines.length - 1
  for (let at = wanted.length - 1; at >= 0; at--) {
    while (index >= 0 && normalized(lines[index]!) === '') index--
    if (index < 0 || normalized(lines[index]!) !== wanted[at]) return null
    if (at > 0) index--
  }
  return index
}

// Send readiness

/** Why Send is off for an address that is not ready (`MailFromAddress.not_ready_reason`). */
export function notReadyText(reason: string | null | undefined): string {
  switch (reason) {
    case 'domain_unverified':
      return 'The domain of this address is not verified yet.'
    case 'inbound_not_live':
      return 'Mail for this domain does not arrive at Orbit yet.'
    case 'dkim_inactive':
      return 'The DKIM record of this domain is not active yet.'
    case 'no_route':
      return 'This domain has no send route.'
    case 'mailbox_suspended':
      return 'This mailbox is suspended.'
    default:
      return 'This address cannot send yet.'
  }
}
