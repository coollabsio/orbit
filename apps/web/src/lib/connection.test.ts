import { expect, test } from 'bun:test'
import { ApiProblem } from '@/api/problem'
import { connectionRetryDelay, createConnectionStore, firstLoadFailed, isConnectionFailure, loadFailed, queryLostConnection, retryConnectionFailure } from './connection'

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

function problem(status: number) {
  return new ApiProblem({ type: 'about:blank', title: 'Request failed', status, code: 'http_error', detail: '', instance: '/', request_id: 'r' })
}

test('a drop shows after a short delay, fails after the long one, and clears when every source is back', async () => {
  const store = createConnectionStore({ showAfterMs: 20, failAfterMs: 80 })
  const phases: string[] = []
  store.subscribe(() => {
    if (phases.at(-1) !== store.getState().phase) phases.push(store.getState().phase)
  })
  const before = Date.now()
  store.report('events', true)
  store.report('chat', true)
  expect(store.getState().phase).toBe('connected')
  const lostAt = store.getState().lostAt
  expect(lostAt).not.toBeNull()
  expect(lostAt!).toBeGreaterThanOrEqual(before)

  await sleep(40)
  expect(store.getState()).toEqual({ phase: 'lost', lostAt })
  await sleep(70)
  expect(store.getState()).toEqual({ phase: 'failed', lostAt })

  // One source back is not the connection back.
  store.report('events', false)
  expect(store.getState().phase).toBe('failed')
  store.report('chat', false)
  expect(store.getState()).toEqual({ phase: 'connected', lostAt: null })
  await sleep(100)
  expect(phases).toEqual(['connected', 'lost', 'failed', 'connected'])
})

test('a blip shorter than the delay never shows', async () => {
  const store = createConnectionStore({ showAfterMs: 30, failAfterMs: 60 })
  let shown = 0
  store.subscribe(() => {
    if (store.getState().phase !== 'connected') shown += 1
  })
  store.report('queries', true)
  await sleep(5)
  store.report('queries', false)
  await sleep(90)
  expect(shown).toBe(0)
  expect(store.getState().lostAt).toBeNull()
})

test('retry calls the listeners and the phase stays failed until a source is back', async () => {
  const store = createConnectionStore({ showAfterMs: 5, failAfterMs: 40 })
  let retries = 0
  const stop = store.onRetry(() => { retries += 1 })
  store.report('events', true)
  await sleep(60)
  expect(store.getState().phase).toBe('failed')
  store.retry()
  expect(retries).toBe(1)
  expect(store.getState().phase).toBe('failed')
  stop()
  store.retry()
  expect(retries).toBe(1)
  store.report('events', false)
  expect(store.getState().phase).toBe('connected')
})

test('core queries retry network and 5xx failures with a bounded backoff, never a 4xx', () => {
  expect(isConnectionFailure(new TypeError('Failed to fetch'))).toBe(true)
  expect(isConnectionFailure(problem(503))).toBe(true)
  expect(isConnectionFailure(problem(429))).toBe(false)
  expect(isConnectionFailure(problem(403))).toBe(false)
  expect([0, 1, 2, 3, 4, 5, 6].map(connectionRetryDelay)).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000])
  expect(retryConnectionFailure(0, problem(502))).toBe(true)
  expect(retryConnectionFailure(5, problem(502))).toBe(true)
  expect(retryConnectionFailure(6, problem(502))).toBe(false)
  expect(retryConnectionFailure(0, problem(404))).toBe(false)
})

test('a query counts as lost while it retries and after it gave up, but not for a 4xx', () => {
  const idle = { data: 1, isError: false, error: null, failureCount: 0, failureReason: null }
  expect(queryLostConnection(idle)).toBe(false)
  expect(queryLostConnection({ ...idle, failureCount: 1, failureReason: problem(500) })).toBe(true)
  expect(queryLostConnection({ ...idle, isError: true, error: new TypeError('Failed to fetch') })).toBe(true)
  expect(queryLostConnection({ ...idle, isError: true, error: problem(403) })).toBe(false)
})

test('a load failed with no data, or when the server refused it; a lost connection with data from before did not', () => {
  const ok = { data: 1, isError: false, error: null, failureCount: 0, failureReason: null }
  expect(loadFailed(ok)).toBe(false)
  expect(loadFailed({ ...ok, isError: true, error: new TypeError('Failed to fetch') })).toBe(false)
  expect(loadFailed({ ...ok, isError: true, error: problem(503) })).toBe(false)
  // Access removed, record deleted, an old tab after a deploy: data from before must not hide it.
  expect(loadFailed({ ...ok, isError: true, error: problem(403) })).toBe(true)
  expect(loadFailed({ ...ok, isError: true, error: problem(409) })).toBe(true)
  expect(loadFailed({ ...ok, data: undefined, isError: true, error: problem(404) })).toBe(true)
  expect(loadFailed({ ...ok, data: undefined, isError: true, error: new TypeError('Failed to fetch') })).toBe(true)
  expect(loadFailed({ ...ok, data: undefined })).toBe(false)
  // A gate also gives up on a first load that failed twice and still retries.
  expect(firstLoadFailed({ ...ok, data: undefined, failureCount: 2 })).toBe(true)
  expect(firstLoadFailed({ ...ok, failureCount: 2 })).toBe(false)
  expect(firstLoadFailed({ ...ok, isError: true, error: problem(409) })).toBe(true)
})
