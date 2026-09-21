export function taskRedirect(searchParams: URLSearchParams): string | null {
  const redirect = searchParams.get('redirect')
  if (!redirect?.startsWith('/') || redirect.startsWith('//')) return null

  const target = new URL(redirect, 'https://orbit.local')
  if (target.origin !== 'https://orbit.local') return null
  return `${target.pathname}${target.search}${target.hash}`
}
