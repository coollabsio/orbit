import { beforeEach, expect, test } from 'bun:test'
import { createAutosaver as createRealAutosaver, type AutosaveOptions, type AutosaveTimers } from './autosave'

/**
 * Fake timers as a local clock passed through `options.timers`: bun's `jest.useFakeTimers()` patches
 * process-wide timers, which leaked into later suite files in trial runs.
 */
function manualClock() {
  let now = 0
  let nextId = 0
  const pending = new Map<number, { at: number; callback: () => void }>()
  const timers: AutosaveTimers = {
    set: (callback, ms) => {
      nextId += 1
      pending.set(nextId, { at: now + ms, callback })
      return nextId
    },
    clear: (handle) => { pending.delete(handle as number) },
  }
  const advanceTimersByTime = (ms: number) => {
    const end = now + ms
    for (;;) {
      const due = [...pending.entries()].filter(([, timer]) => timer.at <= end).sort((a, b) => a[1].at - b[1].at)[0]
      if (!due) break
      pending.delete(due[0])
      now = due[1].at
      due[1].callback()
    }
    now = end
  }
  return { timers, advance: advanceTimersByTime }
}

let clock = manualClock()
beforeEach(() => { clock = manualClock() })
const createAutosaver = <T,>(save: (value: T) => Promise<unknown>, options: AutosaveOptions = {}) =>
  createRealAutosaver(save, { ...options, timers: clock.timers })

/** Let promise callbacks run; the clock only replaces timers. */
const settle = async () => {
  for (let i = 0; i < 10; i += 1) await Promise.resolve()
}

test('rapid changes collapse into one save of the latest value after 500 ms', async () => {
  const saved: number[] = []
  const saver = createAutosaver(async (value: number) => { saved.push(value) })
  saver.schedule(1)
  clock.advance(300)
  saver.schedule(2)
  clock.advance(499)
  await settle()
  expect(saved).toEqual([])
  clock.advance(1)
  await settle()
  expect(saved).toEqual([2])
})

test('a failing save retries with exponential backoff, then waits for the next change', async () => {
  let calls = 0
  const saver = createAutosaver(async () => {
    calls += 1
    throw new Error('offline')
  }, { maxAttempts: 4 })
  saver.schedule('state')
  clock.advance(500)
  await settle()
  expect(calls).toBe(1)
  clock.advance(999)
  await settle()
  expect(calls).toBe(1)
  clock.advance(1)
  await settle()
  expect(calls).toBe(2)
  clock.advance(2_000)
  await settle()
  expect(calls).toBe(3)
  clock.advance(4_000)
  await settle()
  expect(calls).toBe(4)
  clock.advance(120_000)
  await settle()
  expect(calls).toBe(4)

  saver.schedule('newer')
  clock.advance(500)
  await settle()
  expect(calls).toBe(5)
})

test('backoff delays are capped', async () => {
  let calls = 0
  const saver = createAutosaver(async () => {
    calls += 1
    throw new Error('offline')
  }, { baseRetryMs: 1_000, maxRetryMs: 1_500, maxAttempts: 10 })
  saver.schedule('state')
  clock.advance(500)
  await settle()
  clock.advance(1_000)
  await settle()
  expect(calls).toBe(2)
  clock.advance(1_500)
  await settle()
  expect(calls).toBe(3)
  clock.advance(1_500)
  await settle()
  expect(calls).toBe(4)
})

test('a change during a save waits for it and is saved next', async () => {
  const saved: string[] = []
  const finish: Array<() => void> = []
  const saver = createAutosaver((value: string) => new Promise<void>((resolve) => {
    saved.push(value)
    finish.push(resolve)
  }))
  saver.schedule('a')
  clock.advance(500)
  await settle()
  saver.schedule('b')
  clock.advance(500)
  await settle()
  expect(saved).toEqual(['a'])
  finish[0]!()
  await settle()
  expect(saved).toEqual(['a', 'b'])
})

test('a newer change replaces a failed value instead of retrying it', async () => {
  const saved: string[] = []
  let fail = true
  const saver = createAutosaver(async (value: string) => {
    saved.push(value)
    if (fail) throw new Error('offline')
  })
  saver.schedule('old')
  clock.advance(500)
  await settle()
  fail = false
  saver.schedule('new')
  clock.advance(500)
  await settle()
  clock.advance(60_000)
  await settle()
  expect(saved).toEqual(['old', 'new'])
})

test('flush saves at once, onSaved reports success, and dispose stops retries', async () => {
  const saved: string[] = []
  let savedCount = 0
  const saver = createAutosaver(async (value: string) => { saved.push(value) }, { onSaved: () => { savedCount += 1 } })
  saver.schedule('now')
  saver.flush()
  await settle()
  expect(saved).toEqual(['now'])
  expect(savedCount).toBe(1)

  let calls = 0
  const failing = createAutosaver(async () => {
    calls += 1
    throw new Error('offline')
  })
  failing.schedule('x')
  failing.flush()
  failing.dispose()
  await settle()
  clock.advance(60_000)
  await settle()
  expect(calls).toBe(1)
  failing.schedule('ignored after dispose')
  clock.advance(1_000)
  await settle()
  expect(calls).toBe(1)
})

test('a change flushed while a save is in flight is still sent after dispose', async () => {
  const saved: string[] = []
  const finish: Array<() => void> = []
  const saver = createAutosaver((value: string) => new Promise<void>((resolve) => {
    saved.push(value)
    finish.push(resolve)
  }))
  saver.schedule('a')
  clock.advance(500)
  await settle()
  expect(saved).toEqual(['a'])
  // the user changes something else, then navigates away before 'a' finishes
  saver.schedule('b')
  saver.flush()
  saver.dispose()
  finish[0]!()
  await settle()
  expect(saved).toEqual(['a', 'b'])
})

test('a flushed change still goes out after dispose when its debounce already fired', async () => {
  const saved: string[] = []
  const finish: Array<() => void> = []
  const saver = createAutosaver((value: string) => new Promise<void>((resolve) => {
    saved.push(value)
    finish.push(resolve)
  }))
  saver.schedule('a')
  clock.advance(500)
  saver.schedule('b')
  clock.advance(500)
  await settle()
  saver.flush()
  saver.dispose()
  finish[0]!()
  await settle()
  expect(saved).toEqual(['a', 'b'])
})

test('flushing while the only value is in flight does not skip its retry backoff', async () => {
  let calls = 0
  let reject: ((error: Error) => void) | undefined
  const saver = createAutosaver(() => new Promise<void>((_resolve, fail) => {
    calls += 1
    reject = fail
  }))
  saver.schedule('a')
  clock.advance(500)
  await settle()
  saver.flush()
  reject!(new Error('offline'))
  await settle()
  expect(calls).toBe(1)
  clock.advance(1_000)
  await settle()
  expect(calls).toBe(2)
})
