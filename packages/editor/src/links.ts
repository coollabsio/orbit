// The link policy of the page editor, shared by the web and the mobile editor: which hrefs a link may have and open.

/** Control characters and whitespace browsers strip from URLs before parsing the scheme. */
// eslint-disable-next-line no-control-regex
export const URL_NOISE = /[\u0000-\u0020\u007f-\u009f\u00a0\u1680\u180e\u2000-\u2029\u205f\u3000\ufeff]/g

const SAFE_SCHEME = /^(?:https?:\/\/|mailto:)/i
const ANY_SCHEME = /^[a-z][a-z0-9+.-]*:/i
/** An in-app page link, exactly `/docs/<page uuid>` (what the Notion import writes for links between pages). */
const INTERNAL_PAGE_LINK = /^\/docs\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/i

/** The page id of an internal page link (`/docs/<uuid>`), or null for anything else. */
export function internalPageLinkId(href: unknown): string | null {
  if (typeof href !== 'string') return null
  const match = INTERNAL_PAGE_LINK.exec(href)
  return match ? match[1].toLowerCase() : null
}

/** Link policy for the editor: absolute http(s) and mailto hrefs, and exact in-app page links `/docs/<uuid>`. */
export function isSafeLinkHref(href: unknown): href is string {
  if (typeof href !== 'string') return false
  return internalPageLinkId(href) !== null || SAFE_SCHEME.test(href.replace(URL_NOISE, ''))
}

/**
 * Normalizes a stored/pasted href: keeps http(s)/mailto and `/docs/<uuid>` page links, upgrades bare hosts
 * ("example.com/x") to https, and returns null for everything else (javascript:, data:, other relative paths,
 * fragments, other schemes).
 */
export function normalizeLinkHref(href: unknown): string | null {
  if (typeof href !== 'string') return null
  const cleaned = href.replace(URL_NOISE, '')
  if (!cleaned) return null
  if (SAFE_SCHEME.test(cleaned)) return cleaned
  if (cleaned === href && internalPageLinkId(cleaned)) return cleaned
  // Anything that looks like a scheme is rejected, except a dotted host with a port ("example.com:8080/x").
  if (ANY_SCHEME.test(cleaned) && !/^[^:/]*\.[^:/]*:\d+(?:[/?#]|$)/.test(cleaned)) return null
  if (/^[/?#.\\]/.test(cleaned)) return null
  // Bare host: needs a dot in the host part (same rule BlockNote uses for autolinking).
  const host = cleaned.split(/[/?#]/)[0]
  if (!host.includes('.')) return null
  return `https://${cleaned}`
}
