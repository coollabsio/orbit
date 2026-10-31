import { describe, expect, test } from 'bun:test'
import { createOrbitClient } from '@orbit/api/client'

import { collabTicket } from './docs-api'
import { blockKindOf, docsLinkPageId, parseCommand, PROBLEM_RESPONSES, whileMounted } from './page-editor'

function client(respond: (request: Request) => Response | Promise<Response>) {
  const seen: Request[] = []
  const instance = createOrbitClient({
    baseUrl: 'https://orbit.test',
    token: () => 'secret',
    fetch: async (request) => {
      seen.push(request)
      return respond(request)
    },
  })
  return { instance, seen }
}

const problem = (status: number, code: string) =>
  new Response(JSON.stringify({ type: 'about:blank', title: code, status, code, detail: code }), { status, headers: { 'content-type': 'application/problem+json' } })

describe('collabTicket', () => {
  test('asks for a ticket of the page with the bearer token', async () => {
    const { instance, seen } = client(() => Response.json({ ticket: 't-1', expires_at: '2026-10-24T10:01:00Z' }))
    expect(await collabTicket(instance, 'page-1')).toEqual({ ok: true, ticket: 't-1' })
    expect(seen.map((request) => [request.method, request.url, request.headers.get('authorization')])).toEqual([
      ['POST', 'https://orbit.test/api/v1/auth/socket-ticket', 'Bearer secret'],
    ])
    expect(await seen[0].json()).toEqual({ kind: 'page', id: 'page-1' })
  })

  test('an ended session and a failed request report their status; no response is status 0', async () => {
    expect(await collabTicket(client(() => problem(401, 'authentication_required')).instance, 'page-1')).toEqual({ ok: false, status: 401 })
    expect(await collabTicket(client(() => problem(503, 'unavailable')).instance, 'page-1')).toEqual({ ok: false, status: 503 })
    expect(
      await collabTicket(
        client(() => {
          throw new TypeError('Network request failed')
        }).instance,
        'page-1',
      ),
    ).toEqual({ ok: false, status: 0 })
  })
})

test('the toolbar names the block types it can set, and nothing else', () => {
  expect(blockKindOf({ type: 'heading', props: { level: 2, isToggleable: false } })).toBe('heading2')
  expect(blockKindOf({ type: 'checkListItem', props: { checked: true } })).toBe('check')
  expect(blockKindOf({ type: 'paragraph' })).toBe('paragraph')
  expect(blockKindOf({ type: 'image', props: { url: '/x' } })).toBeNull()
  expect(blockKindOf({ type: 'heading', props: { level: 4 } })).toBeNull()
})

test('the screen reconnects after a reset, stops when the page is out of reach, and waits out rate limits', () => {
  expect(PROBLEM_RESPONSES.reset).toEqual({ notice: null, refetch: true, reconnect: true, readOnly: false, stop: false })
  expect(PROBLEM_RESPONSES['read-only']).toEqual({ notice: 'This workspace is read-only.', refetch: true, reconnect: true, readOnly: true, stop: false })
  expect(PROBLEM_RESPONSES.forbidden).toEqual({ notice: 'You no longer have access to this page.', refetch: false, reconnect: false, readOnly: false, stop: true })
  expect(PROBLEM_RESPONSES.gone.stop).toBe(true)
  expect(PROBLEM_RESPONSES.session).toEqual({ notice: 'Your session ended. Sign in again.', refetch: true, reconnect: false, readOnly: false, stop: true })
  expect(PROBLEM_RESPONSES['rate-limited']).toEqual({ notice: null, refetch: false, reconnect: false, readOnly: false, stop: false })
})

test('in-app page links open in the app, on this server or as a path', () => {
  const id = '0199a0b0-0000-7000-8000-000000000001'
  expect(docsLinkPageId(`/docs/${id}`, 'https://orbit.test')).toBe(id)
  expect(docsLinkPageId(`https://orbit.test/docs/${id}#block`, 'https://orbit.test/')).toBe(id)
  expect(docsLinkPageId(`https://elsewhere.test/docs/${id}`, 'https://orbit.test')).toBeNull()
  expect(docsLinkPageId('/docs/not-a-page', 'https://orbit.test')).toBeNull()
})

test('commands from the native side are checked before they run', () => {
  expect(parseCommand({ type: 'style', style: 'bold' })).toEqual({ type: 'style', style: 'bold' })
  expect(parseCommand({ type: 'block', kind: 'heading2' })).toEqual({ type: 'block', kind: 'heading2' })
  expect(parseCommand({ type: 'image', url: '/api/x', name: 'a.png', extra: 1 })).toEqual({ type: 'image', url: '/api/x', name: 'a.png' })
  expect(parseCommand({ type: 'undo', style: 'bold' })).toEqual({ type: 'undo' })
  expect(parseCommand({ type: 'style', style: 'shout' })).toBeNull()
  expect(parseCommand({ type: 'block', kind: 'toString' })).toBeNull()
  expect(parseCommand({ type: 'image', url: 3 })).toBeNull()
  expect(parseCommand('bold')).toBeNull()
  expect(parseCommand(null)).toBeNull()
})

describe('whileMounted', () => {
  const settled = async (promise: Promise<unknown>) => {
    const outcome = await Promise.race([promise.then((value) => ({ value }), (error: Error) => ({ error: error.message })), new Promise((resolve) => setTimeout(() => resolve('pending'), 20))])
    return outcome
  }

  test('passes results and errors through while the editor is mounted', async () => {
    const mounted = { current: true }
    expect(await settled(whileMounted(mounted, async (x: number) => x * 2)(21))).toEqual({ value: 42 })
    expect(await settled(whileMounted(mounted, async () => { throw new Error('offline') })())).toEqual({ error: 'offline' })
  })

  test('never settles once the editor unmounted before the action finished', async () => {
    const mounted = { current: true }
    let finish: (value: string) => void = () => {}
    const call = whileMounted(mounted, () => new Promise<string>((resolve) => (finish = resolve)))()
    mounted.current = false
    finish('ticket')
    expect(await settled(call)).toBe('pending')
    const failing = whileMounted(mounted, async () => { throw new Error('gone') })()
    expect(await settled(failing)).toBe('pending')
  })
})
