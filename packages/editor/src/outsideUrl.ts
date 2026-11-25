/** Orbit's route that loads an outside image for a signed-in user (`GET /api/v1/image?u=…`). */
export const IMAGE_PROXY_PATH = '/api/v1/image'

/**
 * True for a URL on another server. Orbit's own files are relative (`/api/…`), and `blob:` and `data:` URLs come
 * from this device.
 */
export function isOutsideUrl(url: string, origin: string): boolean {
  if (!/^https?:\/\//i.test(url)) return false
  try {
    return new URL(url).origin !== origin
  } catch {
    return true
  }
}

/**
 * The URL to show an image with: an outside image loads through Orbit, so its server sees Orbit's address, not the
 * reader's, and gets no cookie (a tracking pixel learns nothing). Other URLs stay as they are.
 */
export function proxiedImageUrl(url: string, origin: string): string {
  return isOutsideUrl(url, origin) ? `${IMAGE_PROXY_PATH}?u=${encodeURIComponent(url)}` : url
}

/** A video or audio player on another server loads nothing until the reader presses play, so that server does not learn who opened the page. */
export function mediaPreload(url: string, origin: string): 'none' | 'metadata' {
  return isOutsideUrl(url, origin) ? 'none' : 'metadata'
}
