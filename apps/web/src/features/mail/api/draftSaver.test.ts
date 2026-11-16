import { expect, test } from 'bun:test'
import { createDraftSaver, type DraftSaveFailure, type DraftSaverState } from './draftSaver'

interface Draft {
  version: number
  subject?: string
  body_html?: string
}

function setup(initial: Draft | null = { version: 1, subject: '', body_html: '' }) {
  let cached: Draft | undefined = initial ?? undefined
  const sends: Array<{ change: unknown; version: number; resolve: (draft: Draft) => void; reject: (error: unknown) => void }> = []
  const timers = new Map<number, { run: () => void; at: number }>()
  let nextTimer = 1
  let now = 0
  let state: DraftSaverState = { saving: false, problem: null, restore: null }
  let saved = 0
  let failure: DraftSaveFailure<Draft> = { kind: 'other' }
  const saver = createDraftSaver<Draft>({
    read: () => cached,
    write: (draft) => void (cached = draft),
    send: (change, version) => new Promise<Draft>((resolve, reject) => void sends.push({ change, version, resolve, reject })),
    classify: () => failure,
    onState: (next) => void (state = next),
    onSaved: () => void (saved += 1),
    setTimer: (run, ms) => {
      timers.set(nextTimer, { run, at: now + ms })
      return nextTimer++
    },
    clearTimer: (handle) => void timers.delete(handle as number),
    delayMs: 2000,
  })
  const advance = async (ms: number) => {
    now += ms
    for (const [id, timer] of [...timers]) {
      if (timer.at <= now) {
        timers.delete(id)
        timer.run()
      }
    }
    await Promise.resolve()
  }
  const settle = async () => {
    for (let index = 0; index < 5; index += 1) await Promise.resolve()
  }
  return {
    saver,
    sends,
    advance,
    settle,
    state: () => state,
    cached: () => cached,
    setCached: (draft: Draft | undefined) => void (cached = draft),
    saved: () => saved,
    timers: () => timers.size,
    fail: (next: DraftSaveFailure<Draft>) => void (failure = next),
  }
}

test('waits 2 s after the last change and sends the cached version', async () => {
  const t = setup()
  t.saver.save({ subject: 'a' })
  await t.advance(1500)
  t.saver.save({ body_html: 'b' })
  await t.advance(1500)
  expect(t.sends).toHaveLength(0)
  await t.advance(500)
  expect(t.sends).toHaveLength(1)
  expect(t.sends[0]).toMatchObject({ change: { subject: 'a', body_html: 'b' }, version: 1 })
  t.sends[0].resolve({ version: 2, subject: 'a' })
  await t.settle()
  expect(t.cached()?.version).toBe(2)
  expect(t.saved()).toBe(1)
  expect(t.state()).toEqual({ saving: false, problem: null, restore: null })
})

test('never two saves at once; text typed during a save goes out after it, with the new version', async () => {
  const t = setup()
  t.saver.save({ subject: 'a' })
  await t.advance(2000)
  expect(t.sends).toHaveLength(1)
  t.saver.save({ subject: 'ab' })
  await t.advance(2000)
  expect(t.sends).toHaveLength(1)
  t.sends[0].resolve({ version: 2 })
  await t.settle()
  await t.advance(2000)
  expect(t.sends).toHaveLength(2)
  expect(t.sends[1]).toMatchObject({ change: { subject: 'ab' }, version: 2 })
})

test('a draft_conflict puts the server copy in the cache and the text in restore; a second one merges', async () => {
  const t = setup()
  t.fail({ kind: 'conflict', current: { version: 5, subject: 'server' } })
  t.saver.save({ subject: 'mine', mode: 'reply_all' })
  await t.advance(2000)
  t.sends[0].reject(new Error('409'))
  await t.settle()
  expect(t.cached()).toEqual({ version: 5, subject: 'server' })
  expect(t.state().restore).toEqual({ subject: 'mine', mode: 'reply_all' })
  expect(t.state().problem).toBeNull()

  t.fail({ kind: 'conflict', current: { version: 6, subject: 'server 2' } })
  t.saver.save({ body_html: 'more' })
  await t.advance(2000)
  t.sends[1].reject(new Error('409'))
  await t.settle()
  expect(t.state().restore).toEqual({ subject: 'mine', mode: 'reply_all', body_html: 'more' })
  t.saver.dismissRestore()
  expect(t.state().restore).toBeNull()
})

test('a conflict whose server copy already has the text shows no banner', async () => {
  const t = setup()
  t.fail({ kind: 'conflict', current: { version: 3, subject: 'same' } })
  t.saver.save({ subject: 'same' })
  await t.advance(2000)
  t.sends[0].reject(new Error('409'))
  await t.settle()
  expect(t.state().restore).toBeNull()
})

test('draft_sending keeps the text, stops saving until reset, and saving resumes after', async () => {
  const t = setup()
  t.fail({ kind: 'sending' })
  t.saver.save({ subject: 'late' })
  await t.advance(2000)
  t.sends[0].reject(new Error('409'))
  await t.settle()
  expect(t.state()).toMatchObject({ problem: 'sending', restore: { subject: 'late' } })
  t.saver.save({ body_html: 'typed after' })
  await t.advance(5000)
  expect(t.sends).toHaveLength(1)
  expect(t.state().restore).toEqual({ subject: 'late', body_html: 'typed after' })

  t.saver.reset()
  expect(t.state().problem).toBeNull()
  t.saver.applyRestore()
  await t.advance(2000)
  expect(t.sends).toHaveLength(2)
  expect(t.sends[1].change).toEqual({ subject: 'late', body_html: 'typed after' })
})

test('another failure keeps the text and does not retry on a timer', async () => {
  const t = setup()
  t.saver.save({ subject: 'a' })
  await t.advance(2000)
  t.sends[0].reject(new Error('network'))
  await t.settle()
  expect(t.state().problem).toBe('error')
  expect(t.timers()).toBe(0)
  void t.saver.flush()
  await t.settle()
  expect(t.sends[1].change).toEqual({ subject: 'a' })
})

test('dispose flushes what is pending once and nothing runs after', async () => {
  const t = setup()
  t.saver.save({ subject: 'a' })
  t.saver.dispose()
  expect(t.sends).toHaveLength(1)
  expect(t.sends[0]).toMatchObject({ change: { subject: 'a' }, version: 1 })
  expect(t.timers()).toBe(0)
  t.saver.save({ subject: 'b' })
  t.saver.dispose()
  await t.advance(10_000)
  expect(t.sends).toHaveLength(1)
  t.sends[0].resolve({ version: 2 })
  await t.settle()
  expect(t.sends).toHaveLength(1)
})

test('dispose during a save flushes the text typed meanwhile once, after the save', async () => {
  const t = setup()
  t.saver.save({ subject: 'a' })
  await t.advance(2000)
  t.saver.save({ subject: 'ab' })
  t.saver.dispose()
  expect(t.sends).toHaveLength(1)
  t.sends[0].resolve({ version: 2 })
  await t.settle()
  expect(t.sends).toHaveLength(2)
  expect(t.sends[1]).toMatchObject({ change: { subject: 'ab' }, version: 2 })
  await t.advance(10_000)
  expect(t.sends).toHaveLength(2)
})

test('stop after a send drops pending text without an error; a gone draft is not an error either', async () => {
  const t = setup()
  t.saver.save({ subject: 'a' })
  t.saver.stop()
  await t.advance(5000)
  expect(t.sends).toHaveLength(0)
  expect(t.state().problem).toBeNull()
  t.saver.save({ subject: 'b' })
  await t.advance(5000)
  expect(t.sends).toHaveLength(0)
  // Undo seeds the draft again: saving resumes
  t.saver.reset()
  t.saver.save({ subject: 'c' })
  await t.advance(2000)
  expect(t.sends).toHaveLength(1)

  const gone = setup(null)
  gone.saver.save({ subject: 'a' })
  await gone.advance(2000)
  expect(gone.sends).toHaveLength(0)
  expect(gone.state().problem).toBeNull()
})
