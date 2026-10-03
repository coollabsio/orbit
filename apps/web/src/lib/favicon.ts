import { useEffect } from 'react'

export const FAVICON = '/favicon.svg'
export const FAVICON_UNREAD = '/favicon-unread.svg'

/** The tab icon: the plain mark, or the mark with a dot while something is unread. */
export function faviconHref(unreadCount: number): string {
  return unreadCount > 0 ? FAVICON_UNREAD : FAVICON
}

function setFavicon(href: string) {
  const link = document.querySelector<HTMLLinkElement>('link[rel="icon"]')
  if (link && link.getAttribute('href') !== href) link.setAttribute('href', href)
}

/** Shows the dot on the tab icon while `unreadCount` is above zero; the plain icon is back on unmount (sign-out). */
export function useUnreadFavicon(unreadCount: number) {
  const href = faviconHref(unreadCount)
  useEffect(() => {
    setFavicon(href)
    return () => setFavicon(FAVICON)
  }, [href])
}
