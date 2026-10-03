import { appNavigate } from '@/lib/navigateBridge'
import { playSound, type SoundKind } from '@/lib/sounds'
import { appPath, workerRegistration } from './push'

/** The live socket's `notify` signal: the server sends it to the user's tabs with recent input instead of a push. */
export interface Notice {
  title: string
  body: string
  /** An app path, such as `/chat/<id>?thread=<id>` or `/inbox`. */
  url: string
  /** Notices with the same tag replace each other in the system's list. */
  tag: string
  sound: SoundKind
  /** The picture of who caused it, as an app path; absent when they have none. */
  icon?: string | null
}

/** How long the tab that took a notice keeps the others from taking it too. */
const HOLD_MS = 2000
/** How long a window without focus waits before it takes a notice, so the window the user looks at takes it first. */
const YIELD_MS = 150

/** The notice's page is the one this window shows (for a thread: with that thread open). */
export function showsPage(noticeUrl: string, location: Pick<Location, 'pathname' | 'search'>): boolean {
  const target = new URL(noticeUrl, 'http://orbit.invalid')
  const thread = target.searchParams.get('thread')
  return target.pathname === location.pathname && (thread === null || new URLSearchParams(location.search).get('thread') === thread)
}

/** What the tab is and can do; tests put their own in. */
export interface NoticeEnvironment {
  hasFocus: () => boolean
  /** The window shows the page of this notice. */
  viewing: (url: string) => boolean
  /** `undefined` where the browser has no Web Locks. */
  locks: Pick<LockManager, 'request'> | undefined
  permission: () => NotificationPermission | 'unsupported'
  show: (notice: Notice) => Promise<void>
  play: (sound: SoundKind) => void
  holdMs: number
  yieldMs: number
}

/** A short stable hash (djb2) of a text, for a lock name. */
export function shortHash(text: string): string {
  let hash = 5381
  for (let index = 0; index < text.length; index += 1) hash = ((hash * 33) ^ text.charCodeAt(index)) >>> 0
  return hash.toString(36)
}

async function showSystemNotification(notice: Notice) {
  const url = appPath(notice.url)
  const icon = notice.icon?.startsWith('/api/v1/users/') ? notice.icon : '/icon-192.png'
  const worker = await workerRegistration()
  if (worker) {
    // The worker's click handler focuses a window and tells it where to go.
    await worker.showNotification(notice.title, { body: notice.body, tag: notice.tag, data: { url }, icon, badge: '/icon-192.png', silent: true })
    return
  }
  const notification = new Notification(notice.title, { body: notice.body, tag: notice.tag, icon, silent: true })
  notification.onclick = () => {
    window.focus()
    appNavigate(url)
    notification.close()
  }
}

const browserEnvironment: NoticeEnvironment = {
  hasFocus: () => document.hasFocus(),
  viewing: (url) => showsPage(url, window.location),
  get locks() {
    return typeof navigator !== 'undefined' ? navigator.locks : undefined
  },
  permission: () => (typeof Notification === 'undefined' ? 'unsupported' : Notification.permission),
  show: showSystemNotification,
  play: (sound) => playSound(sound),
  holdMs: HOLD_MS,
  yieldMs: YIELD_MS,
}

/**
 * Acts on a `notify` signal in an open tab. The window the user looks at plays the sound only, and nothing when it
 * shows the notice's page already (it still takes the lock, so a tab in the background stays quiet too). A window without focus also shows a system notification where the user allowed
 * them. Of several open tabs only the one that gets the notice's lock acts; the focused one asks first. Resolves to
 * whether this tab acted.
 */
export async function handleNotice(notice: Notice, environment: NoticeEnvironment = browserEnvironment): Promise<boolean> {
  const focused = environment.hasFocus()
  // The user reads the page of the notice here: this tab takes the notice and does nothing, so no other tab acts on it.
  const seen = focused && environment.viewing(notice.url)
  if (!focused) await new Promise((resolve) => setTimeout(resolve, environment.yieldMs))
  const act = async () => {
    if (seen) return
    if (!focused && environment.permission() === 'granted') await environment.show(notice).catch(() => {})
    environment.play(notice.sound)
  }
  const { locks } = environment
  if (!locks) {
    await act()
    return !seen
  }
  const name = `orbit-notify:${notice.tag}:${shortHash(notice.title + notice.body)}`
  return (await locks.request(name, { ifAvailable: true }, async (lock) => {
    if (!lock) return false
    await act()
    // The other tabs get the same signal a moment apart; they must still find the lock taken.
    await new Promise((resolve) => setTimeout(resolve, environment.holdMs))
    return !seen
  })) as boolean
}
