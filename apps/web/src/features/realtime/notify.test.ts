import { expect, test } from 'bun:test'
import { handleNotice, type Notice, type NoticeEnvironment, shortHash, showsPage } from './notify'

const notice: Notice = { title: 'Ada in #general', body: 'Lunch?', url: '/chat/c1', tag: 'chat:c1', sound: 'mention' }

/** A tab: what it showed and played, and the lock names it asked for. `held` is the set of locks other tabs hold. */
function tab(options: { focused?: boolean; viewing?: boolean; permission?: NotificationPermission | 'unsupported'; locks?: boolean; held?: Set<string> } = {}) {
  const shown: Notice[] = []
  const played: string[] = []
  const asked: string[] = []
  const held = options.held ?? new Set<string>()
  const locks = {
    request: async (name: string, _options: LockOptions, callback: (lock: Lock | null) => Promise<unknown>) => {
      asked.push(name)
      if (held.has(name)) return callback(null)
      held.add(name)
      try {
        return await callback({ name, mode: 'exclusive' })
      } finally {
        held.delete(name)
      }
    },
  } as unknown as NoticeEnvironment['locks']
  const environment: NoticeEnvironment = {
    hasFocus: () => options.focused ?? false,
    viewing: () => options.viewing ?? false,
    locks: options.locks === false ? undefined : locks,
    permission: () => options.permission ?? 'granted',
    show: async (shownNotice) => void shown.push(shownNotice),
    play: (sound) => void played.push(sound),
    holdMs: 20,
    yieldMs: 5,
  }
  return { environment, shown, played, asked, held }
}

test('a focused window that shows the page of the notice does nothing', async () => {
  const focused = tab({ focused: true, viewing: true })
  expect(await handleNotice(notice, focused.environment)).toBe(false)
  expect(focused.shown).toEqual([])
  expect(focused.played).toEqual([])
})

test('a tab in the background stays quiet when the focused window shows the page of the notice', async () => {
  const held = new Set<string>()
  const background = tab({ held })
  const reading = tab({ focused: true, viewing: true, held })
  const results = await Promise.all([handleNotice(notice, background.environment), handleNotice(notice, reading.environment)])
  expect(results).toEqual([false, false])
  expect(background.shown).toEqual([])
  expect(background.played).toEqual([])
  expect(reading.played).toEqual([])
})

test('a focused window on another page plays the sound and shows no system notification', async () => {
  const focused = tab({ focused: true })
  expect(await handleNotice(notice, focused.environment)).toBe(true)
  expect(focused.shown).toEqual([])
  expect(focused.played).toEqual(['mention'])
})

test('the focused window takes the notice before a window without focus', async () => {
  const held = new Set<string>()
  const background = tab({ held })
  const focused = tab({ focused: true, held })
  // The background tab gets the signal first, and still the focused one acts.
  const results = await Promise.all([handleNotice(notice, background.environment), handleNotice(notice, focused.environment)])
  expect(results).toEqual([false, true])
  expect(focused.played).toEqual(['mention'])
  expect(background.shown).toEqual([])
  expect(background.played).toEqual([])
})

test('a notice is for the page a window shows when the path is the same, and for a thread when that thread is open', () => {
  expect(showsPage('/chat/c1', { pathname: '/chat/c1', search: '' })).toBe(true)
  expect(showsPage('/chat/c1', { pathname: '/chat/c1', search: '?thread=m1' })).toBe(true)
  expect(showsPage('/chat/c1', { pathname: '/chat/c2', search: '' })).toBe(false)
  expect(showsPage('/chat/c1?thread=m1', { pathname: '/chat/c1', search: '?thread=m1' })).toBe(true)
  expect(showsPage('/chat/c1?thread=m1', { pathname: '/chat/c1', search: '' })).toBe(false)
  expect(showsPage('/inbox', { pathname: '/tasks', search: '' })).toBe(false)
})

test('an unfocused tab that gets the lock shows the notification and plays the sound', async () => {
  const background = tab()
  expect(await handleNotice(notice, background.environment)).toBe(true)
  expect(background.shown).toEqual([notice])
  expect(background.played).toEqual(['mention'])
  expect(background.asked).toEqual([`orbit-notify:chat:c1:${shortHash('Ada in #generalLunch?')}`])
})

test('of two tabs with the same notice only the first acts, and a different notice is not held back', async () => {
  const held = new Set<string>()
  const first = tab({ held })
  const second = tab({ held })
  const results = await Promise.all([handleNotice(notice, first.environment), handleNotice(notice, second.environment)])
  expect(results).toEqual([true, false])
  expect(first.played).toEqual(['mention'])
  expect(second.shown).toEqual([])
  expect(second.played).toEqual([])

  const running = handleNotice(notice, first.environment)
  expect(await handleNotice({ ...notice, body: 'Coffee?', sound: 'message' }, second.environment)).toBe(true)
  expect(second.played).toEqual(['message'])
  await running
})

test('without the permission there is no system notification, only the sound', async () => {
  for (const permission of ['default', 'denied', 'unsupported'] as const) {
    const background = tab({ permission })
    expect(await handleNotice(notice, background.environment)).toBe(true)
    expect(background.shown).toEqual([])
    expect(background.played).toEqual(['mention'])
  }
})

test('a browser without Web Locks acts directly, and a failed notification still plays the sound', async () => {
  const background = tab({ locks: false })
  background.environment.show = async () => {
    throw new Error('no worker')
  }
  expect(await handleNotice(notice, background.environment)).toBe(true)
  expect(background.played).toEqual(['mention'])
})
