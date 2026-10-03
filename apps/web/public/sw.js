// Orbit's service worker: shows pushed notifications and opens Orbit when one is clicked. It caches nothing and has
// no fetch handler, so every request goes to the network as if the worker were not there.

self.addEventListener('install', () => self.skipWaiting())
self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()))

/** An app path only: a pushed `url` never opens another site. */
function appPath(url) {
  if (typeof url !== 'string' || !url.startsWith('/')) return '/'
  try {
    // The parser decides: `/\\host` and a path with a tab in it name another site.
    const parsed = new URL(url, self.location.origin)
    return parsed.origin === self.location.origin ? parsed.pathname + parsed.search + parsed.hash : '/'
  } catch {
    return '/'
  }
}

/** The picture of who caused the notice (a path of the app), or the app's icon. */
function noticeIcon(icon) {
  return typeof icon === 'string' && icon.startsWith('/api/v1/users/') ? icon : '/icon-192.png'
}

self.addEventListener('push', (event) => {
  let notice = {}
  try {
    notice = event.data ? event.data.json() : {}
  } catch {
    // A push that is not JSON still shows a notification: browsers take the permission away after silent pushes.
  }
  const title = typeof notice.title === 'string' && notice.title ? notice.title : 'Orbit'
  event.waitUntil(
    // A visible Orbit window got the same notice and plays Orbit's sound: the system's sound would be a second one.
    // A hidden or frozen one may play nothing, so the system's sound stays.
    self.clients.matchAll({ type: 'window' }).then((windows) =>
      self.registration.showNotification(title, {
        body: typeof notice.body === 'string' ? notice.body : '',
        tag: typeof notice.tag === 'string' ? notice.tag : undefined,
        data: { url: appPath(notice.url) },
        icon: noticeIcon(notice.icon),
        badge: '/icon-192.png',
        silent: windows.some((client) => client.visibilityState === 'visible'),
      }),
    ),
  )
})

const OUTSIDE_APP = /^\/(login|setup|recovery|accept-invitation|oauth)(\/|$)/

self.addEventListener('notificationclick', (event) => {
  event.notification.close()
  const url = appPath(event.notification.data && event.notification.data.url)
  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then(async (windows) => {
      // A window on a page outside the app (sign-in, setup, an invitation) has nobody who takes the message.
      const inApp = windows.filter((client) => !OUTSIDE_APP.test(new URL(client.url).pathname))
      const open = inApp.find((client) => client.focused) || inApp.find((client) => client.visibilityState === 'visible') || inApp[0]
      if (!open) return self.clients.openWindow(url)
      open.postMessage({ type: 'orbit:navigate', url })
      try {
        await open.focus()
      } catch {
        // Some browsers refuse the focus; the page has the message and navigates anyway.
      }
    }),
  )
})
