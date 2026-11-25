/**
 * Custom pages (#94): a workspace's own web pages, framed at `/pages/{slug}`. A path and query after
 * `/pages/{slug}/` go to the page's URL, so a link to one report can be shared; the origin never changes.
 */

/** The page's URL with `rest` (the path after `/pages/{slug}/`) and `search` added, or `null` if that leaves its origin. */
export function customPageUrl(base: string, rest = '', search = ''): string | null {
  let url: URL
  try {
    url = new URL(base)
  } catch {
    return null
  }
  const origin = url.origin
  const path = rest.replace(/^\/+/, '')
  if (path) url.pathname = `${url.pathname.replace(/\/+$/, '')}/${path}`
  for (const [key, value] of new URLSearchParams(search)) url.searchParams.append(key, value)
  return url.origin === origin && url.protocol === 'https:' ? url.toString() : null
}

/** Whether the server still lets the app frame the page: its origin is in `ORBIT_FRAME_ORIGINS`. */
export function canFrame(url: string, frameOrigins: readonly string[]): boolean {
  try {
    return frameOrigins.includes(new URL(url).origin)
  } catch {
    return false
  }
}

/** A page address from a name: lowercase letters, digits and single hyphens, at most 48 characters. */
export function slugOf(name: string): string {
  return name
    .normalize('NFKD')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48)
    .replace(/-+$/, '')
}

export const isSlug = (value: string) => value.length <= 48 && /^[a-z0-9]+(-[a-z0-9]+)*$/.test(value)
