const ORIGIN = 'https://orbit.invalid'
// Going back to a sign-in page after a sign-in would loop or show the wrong page.
const AUTH_PAGES = new Set(['/login', '/login/callback', '/register', '/recovery', '/setup'])

/**
 * Where to go after a sign-in: the Orbit page in `return_to`, or `/`. Never another origin. An OAuth consent link keeps
 * only its request id, so a client cannot add its own parameters.
 */
export function loginDestination(value: string | null): string {
  if (!value?.startsWith('/') || value.startsWith('//') || value.includes('\\') || value.length > 2048 || /\p{Cc}/u.test(value)) return '/'
  const url = new URL(value, ORIGIN)
  if (url.origin !== ORIGIN || AUTH_PAGES.has(url.pathname)) return '/'
  if (url.pathname === '/oauth/consent') {
    const request = url.searchParams.get('request')
    return request && /^[A-Za-z0-9_-]{43}$/.test(request) ? `/oauth/consent?request=${request}` : '/'
  }
  return `${url.pathname}${url.search}${url.hash}`
}

/** The login page, set to come back to `returnTo` (a path with its query and hash) after the sign-in. */
export function loginPath(returnTo: string): string {
  const destination = loginDestination(returnTo)
  return destination === '/' ? '/login' : `/login?return_to=${encodeURIComponent(destination)}`
}
