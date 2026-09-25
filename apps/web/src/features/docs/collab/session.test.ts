import { describe, expect, test } from 'bun:test'
import * as Y from 'yjs'
import { FakeProvider } from '@/test/fakeCollab'
import { COLLAB_FRAGMENT } from './connection'
import {
  CollabSession,
  OFFLINE_AFTER_MS,
  PROBE_INTERVAL_MS,
  RATE_LIMIT_RETRY_MS,
  UNSYNCED_WINDOW_MS,
  collabStatusLabel,
  problemOfClose,
  type CollabProblem,
  type CollabTimers,
} from './session'

function fakeClock() {
  let now = 1_000_000
  let seq = 0
  const timers = new Map<number, { at: number; callback: () => void }>()
  const clock: CollabTimers & { advance: (ms: number) => void; pending: () => number } = {
    now: () => now,
    set: (callback, ms) => {
      seq += 1
      timers.set(seq, { at: now + ms, callback })
      return seq
    },
    clear: (handle) => void timers.delete(handle as number),
    advance: (ms) => {
      const until = now + ms
      for (;;) {
        const next = [...timers.entries()].filter(([, timer]) => timer.at <= until).sort((a, b) => a[1].at - b[1].at)[0]
        if (!next) break
        timers.delete(next[0])
        now = next[1].at
        next[1].callback()
      }
      now = until
    },
    pending: () => timers.size,
  }
  return clock
}

function setup() {
  const clock = fakeClock()
  const doc = new Y.Doc()
  const provider = new FakeProvider(doc)
  const fragment = doc.getXmlFragment(COLLAB_FRAGMENT)
  const statuses: string[] = []
  const problems: CollabProblem[] = []
  let probes = 0
  const session = new CollabSession(
    { doc, provider, fragment },
    {
      timers: clock,
      onChange: (state) => statuses.push(state.status),
      onProblem: (problem) => problems.push(problem),
      onProbe: () => {
        probes += 1
      },
    },
  )
  /** A local edit (typing): any origin other than the provider. */
  const type = (text: string) => doc.transact(() => fragment.insert(fragment.length, [new Y.XmlText(text)]), 'local')
  return { clock, doc, provider, fragment, session, statuses, problems, probes: () => probes, type }
}

describe('CollabSession', () => {
  test('connecting until the first sync, then live and ready', () => {
    const { provider, session, statuses } = setup()
    expect(session.state).toEqual({ status: 'connecting', problem: null, ready: false })
    provider.open()
    expect(session.state.status).toBe('connecting')
    expect(session.state.ready).toBe(false)
    provider.sync()
    expect(session.state).toEqual({ status: 'live', problem: null, ready: true })
    expect(statuses).toEqual(['live'])
  })

  test('a short drop reads "Connecting…", a longer one "Offline", and reconnecting is live again', () => {
    const { clock, provider, session } = setup()
    provider.connectAndSync()
    provider.drop(1013)
    expect(session.state.status).toBe('connecting')
    expect(session.state.ready).toBe(true)
    clock.advance(OFFLINE_AFTER_MS - 1)
    expect(session.state.status).toBe('connecting')
    clock.advance(1)
    expect(session.state.status).toBe('offline')
    expect(collabStatusLabel(session.state)).toBe('Offline — changes will sync')
    provider.connectAndSync()
    expect(session.state.status).toBe('live')
    expect(collabStatusLabel(session.state)).toBe('Live')
  })

  test('a reconnect inside the grace period never shows "Offline"', () => {
    const { clock, provider, session, statuses } = setup()
    provider.connectAndSync()
    provider.drop(1012)
    clock.advance(300)
    provider.connectAndSync()
    clock.advance(OFFLINE_AFTER_MS * 2)
    expect(session.state.status).toBe('live')
    expect(statuses).toEqual(['live', 'connecting', 'live'])
  })

  test('unsynced changes: typed while offline, or just before the drop; cleared by the next sync', () => {
    const { clock, provider, session, type } = setup()
    provider.connectAndSync()
    type('online')
    expect(session.hasUnsyncedChanges()).toBe(false)

    // Typed long before the drop: the server has it.
    clock.advance(UNSYNCED_WINDOW_MS + 1)
    provider.drop()
    expect(session.hasUnsyncedChanges()).toBe(false)
    type('offline')
    expect(session.hasUnsyncedChanges()).toBe(true)
    provider.connectAndSync()
    expect(session.hasUnsyncedChanges()).toBe(false)

    // Typed right before the drop: may have been in flight.
    type('racing')
    clock.advance(100)
    provider.drop()
    expect(session.hasUnsyncedChanges()).toBe(true)
  })

  test('remote updates never count as local changes', () => {
    const { provider, session } = setup()
    provider.connectAndSync()
    provider.drop()
    provider.remote((fragment) => fragment.insert(0, [new Y.XmlText('from the server')]))
    expect(session.hasUnsyncedChanges()).toBe(false)
  })

  test.each([
    [4404, 'gone', 'lost', 'Access lost'],
    [4403, 'forbidden', 'lost', 'Access lost'],
    [4401, 'session', 'lost', 'Access lost'],
    [4409, 'reset', 'reset', 'Connecting…'],
    [4413, 'too-large', 'error', 'Not syncing'],
    [4426, 'protocol', 'error', 'Not syncing'],
    [4400, 'protocol', 'error', 'Not syncing'],
  ] as const)('close %d is terminal: %s → %s', (code, problem, status, label) => {
    const { clock, provider, session, problems } = setup()
    provider.connectAndSync()
    provider.drop(code)
    expect(problems).toEqual([problem])
    expect(session.state.status).toBe(status)
    expect(session.state.problem).toBe(problem)
    expect(collabStatusLabel(session.state)).toBe(label)
    // Terminal states stick: no "Offline" timer, no reconnect of our own.
    clock.advance(60_000)
    expect(session.state.status).toBe(status)
    expect(provider.connectCalls).toBe(0)
  })

  test('4429 backs off and reconnects by itself; edits made meanwhile count as unsynced', () => {
    const { clock, provider, session, problems, type } = setup()
    provider.connectAndSync()
    provider.drop(4429)
    expect(problems).toEqual(['rate-limited'])
    expect(session.state.status).toBe('offline')
    type('while limited')
    expect(session.hasUnsyncedChanges()).toBe(true)
    clock.advance(RATE_LIMIT_RETRY_MS - 1)
    expect(provider.connectCalls).toBe(0)
    clock.advance(1)
    expect(provider.connectCalls).toBe(1)
    provider.connectAndSync()
    expect(session.state).toEqual({ status: 'live', problem: null, ready: true })
    expect(session.hasUnsyncedChanges()).toBe(false)
  })

  test('1012/1013 are transient: no problem reported, y-websocket retries', () => {
    const { provider, problems } = setup()
    provider.connectAndSync()
    provider.drop(1012)
    provider.drop(1013)
    expect(problems).toEqual([])
    expect(problemOfClose(1012)).toBeNull()
    expect(problemOfClose(1013)).toBeNull()
    expect(problemOfClose(1006)).toBeNull()
  })

  test('refused handshakes (HTTP errors) trigger a throttled REST probe', () => {
    const { clock, provider, probes } = setup()
    provider.refuse()
    expect(probes()).toBe(0)
    provider.refuse()
    expect(probes()).toBe(1)
    provider.refuse()
    provider.refuse()
    expect(probes()).toBe(1)
    clock.advance(PROBE_INTERVAL_MS)
    provider.refuse()
    expect(probes()).toBe(2)
    // A socket that opened and then dropped is not a refused handshake.
    provider.connectAndSync()
    clock.advance(PROBE_INTERVAL_MS)
    provider.drop()
    provider.open()
    provider.drop()
    expect(probes()).toBe(2)
  })

  test('fail() (probe saw a 404) stops the provider and reports the problem once', () => {
    const { provider, session, problems } = setup()
    provider.refuse()
    session.fail('gone')
    session.fail('gone')
    expect(problems).toEqual(['gone'])
    expect(provider.destroyed).toBe(true)
    expect(session.state.status).toBe('lost')
  })

  test('destroy() tears down provider and document and ignores later events', () => {
    const { doc, provider, session, problems, statuses } = setup()
    provider.connectAndSync()
    let docDestroyed = false
    doc.on('destroy', () => {
      docDestroyed = true
    })
    session.destroy()
    expect(provider.destroyed).toBe(true)
    expect(docDestroyed).toBe(true)
    provider.drop(4404)
    expect(problems).toEqual([])
    expect(statuses).toEqual(['live'])
  })

  test('a provider that synced before the session subscribed starts live', () => {
    const doc = new Y.Doc()
    const provider = new FakeProvider(doc)
    provider.connectAndSync()
    const session = new CollabSession({ doc, provider, fragment: doc.getXmlFragment(COLLAB_FRAGMENT) })
    expect(session.state).toEqual({ status: 'live', problem: null, ready: true })
    session.destroy()
  })
})
