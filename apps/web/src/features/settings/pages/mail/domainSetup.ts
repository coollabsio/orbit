import type { MailDnsRecord, MailDomain } from '@orbit/api/generated/types.gen'

// The setup of a mail domain as steps in task order: verify, receive, send, reach the inbox.

export type StepKey = 'verify' | 'receive' | 'send' | 'inbox'

/**
 * `done`: every record found. `todo`: a record is not published yet. `problem`: a record has another value.
 * `unknown`: not checked yet, or the DNS did not answer. `blocked`: Orbit cannot say which record to publish.
 */
export type StepState = 'done' | 'todo' | 'problem' | 'unknown' | 'blocked'

export interface SetupStep {
  key: StepKey
  title: string
  /** For the title's tooltip: what the step unlocks. */
  help: string
  records: MailDnsRecord[]
  state: StepState
  /** Short result next to the title ("Found", "Not found yet"). */
  summary: string
  /** Recommended, not needed for mail to work. */
  optional: boolean
}

type DomainForSetup = Pick<MailDomain, 'name' | 'records' | 'checked_at' | 'dkim_rotating' | 'provider'>

function stepState(records: MailDnsRecord[]): StepState {
  const states = records.map((record) => record.status.status)
  if (states.includes('wrong')) return 'problem'
  if (states.includes('missing')) return 'todo'
  if (states.includes('unknown')) return 'unknown'
  return 'done'
}

function summaryOf(state: StepState, records: MailDnsRecord[], checked: boolean): string {
  switch (state) {
    case 'done':
      return records.length > 1 ? 'All found' : 'Found'
    case 'problem':
      return 'Wrong value'
    case 'unknown':
      return checked ? 'The DNS did not answer' : 'Not checked yet'
    case 'blocked':
      return 'Nothing to publish yet'
    case 'todo': {
      const found = records.filter((record) => record.status.status === 'ok').length
      return found > 0 ? `${found} of ${records.length} found` : 'Not found yet'
    }
  }
}

/** The domain's setup steps, each with its records and its state at the last check. */
export function setupSteps(domain: DomainForSetup): SetupStep[] {
  const checked = Boolean(domain.checked_at)
  const of = (...kinds: MailDnsRecord['kind'][]) => domain.records.filter((record) => kinds.includes(record.kind))
  const step = (key: StepKey, title: string, help: string, records: MailDnsRecord[], optional = false, blocked: string | null = null): SetupStep => {
    const state = blocked ? 'blocked' : stepState(records)
    return { key, title, help, records, state, summary: blocked ?? summaryOf(state, records, checked), optional }
  }
  const mx = of('mx')
  const dkim = of('dkim')
  const send = step('send', 'Send mail', 'Orbit signs every message it sends with this DKIM key. Without the record, mail from this domain is not sent.', dkim, false, dkim.length === 0 ? 'No DKIM key yet' : null)
  return [
    step('verify', 'Verify the domain', `Proves that ${domain.name} is yours. Orbit receives mail for it once this record is found.`, of('verify')),
    // without a provider, the MX record names this server, which is known only with ORBIT_MAIL_HOSTNAME
    step('receive', 'Receive mail', `Sends mail for ${domain.name} to ${domain.provider ? domain.provider.name : 'this Orbit server'}.`, mx, false, mx.length === 0 ? 'Mail host name not set' : null),
    // during a rotation the old key still signs: the new record is something to do, not a problem
    domain.dkim_rotating && send.state !== 'done' ? { ...send, state: 'todo', summary: 'Publish the new key' } : send,
    step('inbox', 'Reach the inbox', 'Recommended. SPF and DMARC tell other mail servers to trust mail from this domain, so it does not go to spam.', of('spf', 'dmarc'), true),
  ]
}

/** The first step that is not done: it opens first. `null` when all are done. */
export const firstOpenStep = (steps: SetupStep[]): StepKey | null => steps.find((step) => step.state !== 'done')?.key ?? null

/**
 * The record's name as DNS panels want it: relative to the zone (`_orbit-verify`, `@` for the zone itself).
 * Most panels add the zone; with the full name they make `_orbit-verify.example.com.example.com`.
 */
export function relativeHost(name: string, zone: string): string {
  const full = name.toLowerCase().replace(/\.$/, '')
  const apex = zone.toLowerCase().replace(/\.$/, '')
  if (full === apex) return '@'
  return full.endsWith(`.${apex}`) ? full.slice(0, -(apex.length + 1)) : full
}

/** The zone of the domain's records: the one found by the last check, else the domain itself. */
export const zoneOf = (domain: Pick<MailDomain, 'name' | 'dns_zone'>) => domain.dns_zone ?? domain.name

const quoted = (part: string) => `"${part.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`

/** The records as BIND zone-file lines, for the zone import of Cloudflare, Route 53 and others. */
export function zoneFile(domain: Pick<MailDomain, 'name' | 'records'>, ttl = 3600): string {
  const lines = domain.records.map((record) => {
    const value = record.type === 'MX'
      ? `${record.priority ?? 10} ${record.value.replace(/\.$/, '')}.`
      : (record.txt_strings.length > 0 ? record.txt_strings : [record.value]).map(quoted).join(' ')
    return `${record.name.replace(/\.$/, '')}.\t${ttl}\tIN\t${record.type}\t${value}`
  })
  return [`; Orbit mail records for ${domain.name}`, ...lines, ''].join('\n')
}

export interface DnsHost {
  name: string
  /** Where its DNS records are edited; `null` when there is no page to link to. */
  url: string | null
}

/** Known DNS hosts by a part of their name servers' names. */
const DNS_HOSTS: { match: RegExp; name: string; url: string | null }[] = [
  // the `:account/:zone` deep link asks for the account and zone, then opens the DNS records
  { match: /\.ns\.cloudflare\.com$/, name: 'Cloudflare', url: 'https://dash.cloudflare.com/?to=/:account/:zone/dns/records' },
  { match: /\.awsdns-\d+\./, name: 'Amazon Route 53', url: 'https://console.aws.amazon.com/route53/v2/hostedzones' },
  { match: /^ns-cloud-[a-z]\d+\.googledomains\.com$/, name: 'Google Cloud DNS', url: 'https://console.cloud.google.com/net-services/dns/zones' },
  { match: /\.azure-dns\.(com|net|org|info)$/, name: 'Azure DNS', url: 'https://portal.azure.com/#browse/Microsoft.Network%2FdnsZones' },
  { match: /\.registrar-servers\.com$/, name: 'Namecheap', url: 'https://ap.www.namecheap.com/domains/list/' },
  { match: /\.domaincontrol\.com$/, name: 'GoDaddy', url: 'https://dcc.godaddy.com/control/portfolio' },
  { match: /\.porkbun\.com$/, name: 'Porkbun', url: 'https://porkbun.com/account/domainsSpeedy' },
  { match: /\.digitalocean\.com$/, name: 'DigitalOcean', url: 'https://cloud.digitalocean.com/networking/domains' },
  { match: /\.(hetzner\.com|hetzner\.de|your-server\.de)$/, name: 'Hetzner', url: 'https://dns.hetzner.com/' },
  { match: /\.vercel-dns\.com$/, name: 'Vercel', url: 'https://vercel.com/dashboard/domains' },
  { match: /\.gandi\.net$/, name: 'Gandi', url: 'https://admin.gandi.net/domain/' },
  { match: /\.ovh\.net$/, name: 'OVHcloud', url: 'https://www.ovh.com/manager/' },
  { match: /\.name\.com$/, name: 'Name.com', url: 'https://www.name.com/account/domain' },
  { match: /\.ui-dns\.(com|de|org|biz)$/, name: 'IONOS', url: 'https://my.ionos.com/domains' },
  { match: /\.dns-parking\.com$/, name: 'Hostinger', url: 'https://hpanel.hostinger.com/domains' },
  { match: /\.linode\.com$/, name: 'Akamai (Linode)', url: 'https://cloud.linode.com/domains' },
  { match: /\.bunny\.net$/, name: 'Bunny DNS', url: 'https://dash.bunny.net/dns' },
  { match: /\.desec\.(io|org)$/, name: 'deSEC', url: 'https://desec.io/domains' },
]

/** The domain's DNS host from its name servers; an unknown host is named by its servers' domain. `null` without servers. */
export function dnsHost(nameservers: string[]): DnsHost | null {
  const first = nameservers[0]
  if (!first) return null
  const known = DNS_HOSTS.find((host) => nameservers.some((server) => host.match.test(server)))
  if (known) return { name: known.name, url: known.url }
  // `ns1.example.net` → `example.net`
  return { name: first.split('.').slice(-2).join('.'), url: null }
}
