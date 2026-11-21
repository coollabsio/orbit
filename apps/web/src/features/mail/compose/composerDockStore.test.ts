import { expect, test } from 'bun:test'
import {
  closeWindow,
  createComposerDockStore,
  dockCapacity,
  dockStorageKey,
  dropMissing,
  EMPTY_DOCK,
  expandWindow,
  layoutDock,
  minimizeWindow,
  openWindow,
  parseDock,
  serializeDock,
  type DockState,
} from './composerDockStore'

function memoryStorage(initial: Record<string, string> = {}) {
  const items = new Map(Object.entries(initial))
  return {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => void items.set(key, value),
    removeItem: (key: string) => void items.delete(key),
    items,
  }
}

const states = (state: DockState) => Object.fromEntries(state.windows.map((window) => [window.draftId, window.state]))

test('fourth_window_minimizes_oldest', () => {
  let state = EMPTY_DOCK
  for (const id of ['a', 'b', 'c']) state = openWindow(state, id)
  expect(states(state)).toEqual({ a: 'open', b: 'open', c: 'open' })
  state = openWindow(state, 'd')
  expect(states(state)).toEqual({ a: 'minimized', b: 'open', c: 'open', d: 'open' })
  // "oldest" is the one opened longest ago: bringing a back makes b the oldest
  state = openWindow(state, 'a')
  expect(states(state)).toEqual({ a: 'open', b: 'minimized', c: 'open', d: 'open' })
  // the order on screen stays the order they were first opened
  expect(state.windows.map((window) => window.draftId)).toEqual(['a', 'b', 'c', 'd'])
})

test('full screen counts as open; only one window is full screen', () => {
  let state = EMPTY_DOCK
  for (const id of ['a', 'b', 'c']) state = openWindow(state, id)
  state = expandWindow(state, 'a')
  state = expandWindow(state, 'b')
  expect(states(state)).toEqual({ a: 'open', b: 'full', c: 'open' })
  // a and b were opened (expanded) after c: c is the oldest
  state = expandWindow(state, 'd')
  expect(states(state)).toEqual({ a: 'open', b: 'open', c: 'minimized', d: 'full' })
  // opening a full window again brings it back to a window
  state = openWindow(state, 'd')
  expect(state.windows.find((window) => window.draftId === 'd')?.state).toBe('open')
})

test('minimize and close', () => {
  let state = openWindow(openWindow(EMPTY_DOCK, 'a'), 'b')
  state = minimizeWindow(state, 'a')
  expect(states(state)).toEqual({ a: 'minimized', b: 'open' })
  state = closeWindow(state, 'a')
  expect(states(state)).toEqual({ b: 'open' })
  // unknown ids change nothing
  expect(minimizeWindow(state, 'zz')).toBe(state)
  expect(closeWindow(state, 'zz')).toBe(state)
})

test('persists_and_restores', () => {
  const storage = memoryStorage()
  const store = createComposerDockStore({ workspaceId: 'w1', storage })
  store.open('a')
  store.open('b')
  store.minimize('a')
  store.expand('b')
  expect(storage.items.has(dockStorageKey('w1'))).toBe(true)
  expect(dockStorageKey('w1')).toBe('orbit.mail.dock.w1')

  const again = createComposerDockStore({ workspaceId: 'w1', storage })
  // a reload keeps the windows; a full screen one comes back as a window
  expect(states(again.getSnapshot())).toEqual({ a: 'minimized', b: 'open' })
  expect(again.restoredIds()).toEqual(['a', 'b'])
  // the next window still minimizes the oldest after the reload
  again.open('c')
  again.open('d')
  again.open('e')
  expect(states(again.getSnapshot())).toEqual({ a: 'minimized', b: 'minimized', c: 'open', d: 'open', e: 'open' })

  // each workspace has its own dock
  expect(createComposerDockStore({ workspaceId: 'w2', storage }).getSnapshot().windows).toEqual([])
})

test('a broken or foreign stored value restores an empty dock', () => {
  expect(parseDock(null)).toEqual(EMPTY_DOCK)
  expect(parseDock('not json')).toEqual(EMPTY_DOCK)
  expect(parseDock('{"v":99,"windows":[]}')).toEqual(EMPTY_DOCK)
  expect(parseDock(JSON.stringify({ v: 1, windows: [{ id: 'a', state: 'open', at: 1 }, { id: 3 }, { id: 'a', state: 'open', at: 2 }, { id: 'b', state: 'weird', at: 2 }] })).windows).toEqual([
    { draftId: 'a', state: 'open', openedAt: 1, animate: false },
    { draftId: 'b', state: 'open', openedAt: 2, animate: false },
  ])
  const state = openWindow(openWindow(EMPTY_DOCK, 'a'), 'b')
  expect(parseDock(serializeDock(state))).toEqual(state)
  // the open animation is not kept: a reload shows windows at once
  expect(parseDock(serializeDock(openWindow(EMPTY_DOCK, 'a', { animate: true }))).windows[0]?.animate).toBe(false)
})

test('drops_missing_drafts', () => {
  let state = EMPTY_DOCK
  for (const id of ['a', 'b', 'c']) state = openWindow(state, id)
  expect(states(dropMissing(state, new Set(['a', 'c'])))).toEqual({ a: 'open', c: 'open' })
  // only the windows restored at load are checked: one opened since then is not in the list read before it
  expect(states(dropMissing(state, new Set(['a']), new Set(['a', 'b'])))).toEqual({ a: 'open', c: 'open' })

  const storage = memoryStorage()
  const first = createComposerDockStore({ workspaceId: 'w1', storage })
  first.open('a')
  first.open('gone')
  const store = createComposerDockStore({ workspaceId: 'w1', storage })
  store.open('new')
  store.dropMissing(['a'])
  expect(states(store.getSnapshot())).toEqual({ a: 'open', new: 'open' })
  expect(states(createComposerDockStore({ workspaceId: 'w1', storage }).getSnapshot())).toEqual({ a: 'open', new: 'open' })
})

test('subscribers hear every change', () => {
  const store = createComposerDockStore({ workspaceId: 'w1', storage: memoryStorage() })
  let calls = 0
  const stop = store.subscribe(() => void (calls += 1))
  store.open('a')
  store.minimize('a')
  store.minimize('a')
  stop()
  store.close('a')
  expect(calls).toBe(2)
})

test('the layout shows the newest open windows that fit; the rest are pills', () => {
  let state = EMPTY_DOCK
  for (const id of ['a', 'b', 'c']) state = openWindow(state, id)
  state = minimizeWindow(openWindow(state, 'd'), 'd')
  // a minimized already, d minimized now
  expect(layoutDock(state, 3)).toEqual({ windows: ['b', 'c'], pills: ['a', 'd'], full: null })
  expect(layoutDock(state, 1)).toEqual({ windows: ['c'], pills: ['a', 'b', 'd'], full: null })
  state = expandWindow(state, 'b')
  expect(layoutDock(state, 1)).toEqual({ windows: ['c'], pills: ['a', 'd'], full: 'b' })
})

test('dock capacity follows the viewport width', () => {
  // the windows leave the bottom left to the undo toast
  expect(dockCapacity(375)).toBe(1)
  expect(dockCapacity(1280)).toBe(1)
  expect(dockCapacity(1600)).toBe(2)
  expect(dockCapacity(1920)).toBe(2)
  expect(dockCapacity(2400)).toBe(3)
  expect(dockCapacity(4000)).toBe(3)
})

test('only a click animates the open; keys and full screen and back do not', () => {
  const animated = (state: DockState, id: string) => state.windows.find((window) => window.draftId === id)?.animate
  // a click (a pill, Show, Reply)
  let state = openWindow(EMPTY_DOCK, 'a', { animate: true })
  expect(animated(state, 'a')).toBe(true)
  // a key (C, ⌘K, R, Undo): no options
  state = openWindow(state, 'b')
  expect(animated(state, 'b')).toBe(false)
  // minimized, then brought back by a key: at once
  state = openWindow(minimizeWindow(state, 'a'), 'a')
  expect(animated(state, 'a')).toBe(false)
  // full screen and back, even by a click: the same window, no open animation
  state = openWindow(expandWindow(state, 'b'), 'b', { animate: true })
  expect(animated(state, 'b')).toBe(false)
  // another window going full screen sends the full one back without the animation
  state = expandWindow(openWindow(expandWindow(state, 'a'), 'c', { animate: true }), 'c')
  expect(animated(state, 'a')).toBe(false)
  expect(state.windows.find((window) => window.draftId === 'c')?.state).toBe('full')
})
