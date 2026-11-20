import { expect, test } from 'bun:test'

import { absoluteViewUrl, frameDocument, frameEvent, isOpenableLink, metaPolicy, policyNonce } from './mail-frame'

// the header `csp_header` in crates/mail/src/html.rs writes
const HEADER =
  "sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox; default-src 'none'; script-src 'nonce-abc123'; img-src https://orbit.test data:; style-src 'unsafe-inline'; font-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors https://orbit.test"

test('the meta policy keeps every directive but sandbox and frame-ancestors', () => {
  expect(metaPolicy(HEADER, 'https://orbit.test')).toBe(
    "default-src 'none'; script-src 'nonce-abc123'; img-src https://orbit.test data:; style-src 'unsafe-inline'; font-src 'none'; form-action 'none'; base-uri 'none'",
  )
})

test("'self' in img-src becomes the server: a srcdoc frame has no origin of its own", () => {
  expect(metaPolicy("img-src 'self' data:; script-src 'self'", 'http://10.0.0.2:18080')).toBe("img-src http://10.0.0.2:18080 data:; script-src 'self'")
})

test('the nonce is read from script-src', () => {
  expect(policyNonce("script-src 'nonce-q+/x=_-1'")).toBe('q+/x=_-1')
  expect(policyNonce("script-src 'none'")).toBe(null)
})

test('the policy, no-referrer and the link script are the first children of <head>', () => {
  const html = '<!doctype html><html data-orbit-plain><head><style>p{}</style></head><body><p>Hi</p><script nonce="abc123">x()</script></body></html>'
  const framed = frameDocument(html, HEADER, 'https://orbit.test')
  const head = framed.slice(framed.indexOf('<head>') + '<head>'.length)
  expect(head).toStartWith(`<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-abc123';`)
  expect(head.indexOf('<meta name="referrer" content="no-referrer">')).toBeLessThan(head.indexOf('<style>'))
  expect(head.indexOf('<script nonce="abc123">addEventListener(\'click\'')).toBeLessThan(head.indexOf('<style>'))
  expect(framed).not.toContain('frame-ancestors')
  expect(framed).not.toContain('sandbox allow-scripts')
  expect(framed).toContain('<p>Hi</p><script nonce="abc123">x()</script>')
})

test('a document without <head> gets one; quotes in the policy are escaped', () => {
  expect(frameDocument('<p>x</p>', 'img-src "x"', 'https://o.test')).toBe('<head><meta http-equiv="Content-Security-Policy" content="img-src &quot;x&quot;"><meta name="referrer" content="no-referrer"></head><p>x</p>')
  expect(frameDocument('<html><body>y</body></html>', "script-src 'nonce-n'", 'https://o.test')).toStartWith('<html><head><meta')
})

test('root-relative mail URLs get the server in front', () => {
  const framed = frameDocument('<head></head><img src="/api/v1/mail/parts/t/1"><img src="/other">', "img-src 'self'", 'http://h:1')
  expect(framed).toContain('<img src="http://h:1/api/v1/mail/parts/t/1">')
  expect(framed).toContain('<img src="/other">')
})

test('frame posts: size, theme and blocked images, or a link to open', () => {
  expect(frameEvent({ orbitMailHeight: 40.2, orbitMailPlain: true, orbitMailBlocked: 3, orbitMailTheme: 'dark' })).toEqual({ kind: 'size', height: 80, plain: true, blocked: 3, theme: 'dark' })
  expect(frameEvent({ orbitMailHeight: 1e9, orbitMailPlain: 'yes', orbitMailBlocked: -1, orbitMailTheme: 'blue' })).toEqual({ kind: 'size', height: 20_000, plain: false, blocked: null, theme: null })
  expect(frameEvent({ orbitMailOpen: 'https://example.com' })).toEqual({ kind: 'open', url: 'https://example.com' })
  expect(frameEvent({ orbitMailHeight: Number.NaN })).toBe(null)
  expect(frameEvent('hello')).toBe(null)
  expect(frameEvent(null)).toBe(null)
})

test('only web, mail and phone links leave the app', () => {
  expect(isOpenableLink('https://example.com/a')).toBe(true)
  expect(isOpenableLink('mailto:ana@example.com')).toBe(true)
  expect(isOpenableLink('tel:+3612345')).toBe(true)
  expect(isOpenableLink('javascript:alert(1)')).toBe(false)
  expect(isOpenableLink('orbit://auth/callback')).toBe(false)
  expect(isOpenableLink('file:///etc/passwd')).toBe(false)
})

test('a root-relative view URL gets the server', () => {
  expect(absoluteViewUrl('/api/v1/mail/content/t', 'https://orbit.test/')).toBe('https://orbit.test/api/v1/mail/content/t')
  expect(absoluteViewUrl('https://mail.orbit.test/api/v1/mail/content/t', 'https://orbit.test')).toBe('https://mail.orbit.test/api/v1/mail/content/t')
})
