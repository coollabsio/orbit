export function consumeQueryToken(
  href: string,
  replace: (href: string) => void,
): string | null {
  const url = new URL(href)
  const token = url.searchParams.get('token')
  if (!token) return null
  url.searchParams.delete('token')
  replace(`${url.pathname}${url.search}${url.hash}`)
  return token
}
