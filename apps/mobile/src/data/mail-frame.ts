/**
 * The message frame of the thread screen, as data. The server's view document (`/api/v1/mail/content/{token}`) says
 * `frame-ancestors <public origin>`, so the app cannot frame its URL. The app fetches it natively and the DOM component
 * frames the text (`srcdoc`) in the same sandbox as the web (`allow-scripts allow-popups allow-popups-to-escape-sandbox`,
 * never `allow-same-origin`: the frame keeps an opaque origin). The header's CSP moves into a `<meta>`, so the same
 * policy holds. No React.
 */

/** The height limits of a message frame, in pixels (as on the web). */
export const FRAME_MIN_HEIGHT = 80
export const FRAME_MAX_HEIGHT = 20_000

/** Directives a `<meta>` CSP cannot carry (the browser ignores them there); the iframe's own attribute sandboxes it. */
const HEADER_ONLY = new Set(['sandbox', 'frame-ancestors', 'report-uri', 'report-to'])

/**
 * The header's policy for a `<meta>`: without `sandbox` and `frame-ancestors`, and with `'self'` in `img-src` replaced
 * by the server (a `srcdoc` frame has no origin of its own, and the server uses `'self'` when it has no public origin).
 */
export function metaPolicy(header: string, server: string): string {
  return header
    .split(';')
    .map((directive) => directive.trim())
    .filter((directive) => directive !== '' && !HEADER_ONLY.has(directive.split(/\s+/)[0]!.toLowerCase()))
    .map((directive) => (/^img-src\s/i.test(directive) ? directive.replace(/'self'/g, server) : directive))
    .join('; ')
}

/** The script nonce of the policy (`script-src 'nonce-…'`), or `null`. */
export function policyNonce(policy: string): string | null {
  return /'nonce-([A-Za-z0-9+/=_-]+)'/.exec(policy)?.[1] ?? null
}

const escapeAttribute = (value: string) => value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

/**
 * A tap on a link of the mail posts its address to the frame's parent instead of navigating: the app opens it in the
 * system browser. The web view cannot open the frame's popups itself. It runs with the document's own nonce.
 */
const LINK_SCRIPT =
  "addEventListener('click',e=>{const a=e.target&&e.target.closest&&e.target.closest('a[href]');if(!a)return;e.preventDefault();parent.postMessage({orbitMailOpen:a.href},'*')},true)"

/**
 * The document to frame: the policy (`metaPolicy`), `no-referrer` and the link script go first in `<head>`, before
 * anything of the mail. Root-relative mail URLs (`/api/v1/mail/…`, when the server has no public origin) get the
 * server in front, since the frame has no base.
 */
export function frameDocument(html: string, header: string, server: string): string {
  const policy = metaPolicy(header, server)
  const nonce = policyNonce(policy)
  const head =
    `<meta http-equiv="Content-Security-Policy" content="${escapeAttribute(policy)}">` +
    '<meta name="referrer" content="no-referrer">' +
    (nonce ? `<script nonce="${escapeAttribute(nonce)}">${LINK_SCRIPT}</script>` : '')
  const body = html.replace(/(\s(?:src|href)=")(\/api\/v1\/mail\/)/g, `$1${server}$2`)
  const opening = /<head(\s[^>]*)?>/i.exec(body)
  if (opening) return body.slice(0, opening.index + opening[0].length) + head + body.slice(opening.index + opening[0].length)
  const htmlTag = /<html(\s[^>]*)?>/i.exec(body)
  if (htmlTag) return body.slice(0, htmlTag.index + htmlTag[0].length) + `<head>${head}</head>` + body.slice(htmlTag.index + htmlTag[0].length)
  return `<head>${head}</head>${body}`
}

/** What the frame posts (the server's FRAME_SCRIPT), or a link the link script asks to open. */
export type FrameEvent =
  | { kind: 'size'; height: number; plain: boolean; blocked: number | null; theme: 'dark' | 'light' | null }
  | { kind: 'open'; url: string }

/** A post of the frame, or `null` for anything else. The caller has checked that it came from the frame's window. */
export function frameEvent(data: unknown): FrameEvent | null {
  if (!data || typeof data !== 'object') return null
  const value = data as Record<string, unknown>
  if (typeof value.orbitMailOpen === 'string') return { kind: 'open', url: value.orbitMailOpen }
  const height = value.orbitMailHeight
  if (typeof height !== 'number' || !Number.isFinite(height)) return null
  const blocked = value.orbitMailBlocked
  return {
    kind: 'size',
    height: Math.min(FRAME_MAX_HEIGHT, Math.max(FRAME_MIN_HEIGHT, Math.ceil(height))),
    plain: value.orbitMailPlain === true,
    blocked: typeof blocked === 'number' && Number.isInteger(blocked) && blocked >= 0 ? blocked : null,
    theme: value.orbitMailTheme === 'dark' || value.orbitMailTheme === 'light' ? value.orbitMailTheme : null,
  }
}

/** The links a mail may open in the system: web pages, mail and phone numbers. */
export function isOpenableLink(url: string): boolean {
  return /^(https?:\/\/|mailto:|tel:)/i.test(url.trim())
}

/** The view URL as a full URL (root-relative when the server has no public origin). */
export const absoluteViewUrl = (url: string, server: string) => (url.startsWith('/') ? `${server.replace(/\/+$/, '')}${url}` : url)
