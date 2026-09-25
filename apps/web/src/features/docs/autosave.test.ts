import { describe, expect, test } from 'bun:test'
import type { Page } from '@/api/generated/types.gen'
import { PageAutosaver, type AutosaveTimers, type PagePatch, type SaveStatus } from './autosave'

/** Manual clock: timers only fire on `advance`. */
function fakeTimers() {
  let now = 0
  let seq = 0
  const queue = new Map<number, { at: number; callback: () => void }>()
  const timers: AutosaveTimers = {
    set: (callback, ms) => {
      seq += 1
      queue.set(seq, { at: now + ms, callback })
      return seq
    },
    clear: (handle) => {
      queue.delete(handle as number)
    },
  }
  return {
    timers,
    pendingTimers: () => queue.size,
    advance(ms: number) {
      now += ms
      for (const [id, timer] of [...queue].sort((a, b) => a[1].at - b[1].at)) {
        if (timer.at <= now) {
          queue.delete(id)
          timer.callback()
        }
      }
    },
  }
}

const page = (version: number, patch: PagePatch = {}): Page => ({
  id: 'page-1',
  workspace_id: 'workspace-1',
  parent_id: null,
  teamspace_id: 'teamspace-1',
  private: false,
  title: typeof patch.title === 'string' ? patch.title : '',
  icon: null,
  cover_url: null,
  cover_position: null,
  content: (patch.content as unknown[] | undefined) ?? [],
  position: 0,
  creator_id: 'user-1',
  updated_by: 'user-1',
  created_at: '2026-09-25T10:00:00Z',
  updated_at: '2026-09-25T10:00:00Z',
  deleted_at: null,
  version,
})

/** A save function whose requests resolve only when the test says so. */
function controlledSave() {
  const calls: Array<{ patch: PagePatch; version: number; resolve: (page: Page) => void; reject: (error: unknown) => void }> = []
  const save = (patch: PagePatch, version: number) =>
    new Promise<Page>((resolve, reject) => {
      calls.push({ patch, version, resolve, reject })
    })
  return { calls, save }
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0))

class Conflict extends Error {
  readonly currentVersion: number
  readonly current: Page | null
  constructor(currentVersion: number, current: Page | null = null) {
    super('conflict')
    this.currentVersion = currentVersion
    this.current = current
  }
}

function setup(version = 3, rebaseOnto?: (current: Page) => boolean) {
  const clock = fakeTimers()
  const server = controlledSave()
  const statuses: SaveStatus[] = []
  const saved: Page[] = []
  const saver = new PageAutosaver({
    version,
    delay: 800,
    timers: clock.timers,
    save: server.save,
    onSaved: (result) => saved.push(result),
    onStateChange: (state) => statuses.push(state.status),
    conflictOf: (error) => (error instanceof Conflict ? { currentVersion: error.currentVersion, current: error.current } : null),
    rebaseOnto,
  })
  return { clock, server, statuses, saved, saver }
}

describe('PageAutosaver', () => {
  test('debounces edits: one save 800 ms after the last change, with the merged patch', async () => {
    const { clock, server, saver, statuses } = setup()
    saver.update({ title: 'R' })
    clock.advance(500)
    saver.update({ title: 'Ro' })
    clock.advance(500)
    saver.update({ content: [{ type: 'paragraph' }] })
    clock.advance(799)
    expect(server.calls).toHaveLength(0)
    expect(saver.state.status).toBe('pending')

    clock.advance(1)
    expect(server.calls).toHaveLength(1)
    expect(server.calls[0].patch).toEqual({ title: 'Ro', content: [{ type: 'paragraph' }] })
    expect(server.calls[0].version).toBe(3)
    expect(saver.state.status).toBe('saving')

    server.calls[0].resolve(page(4))
    await tick()
    expect(saver.state.status).toBe('saved')
    expect(saver.version).toBe(4)
    expect(saver.hasUnsavedChanges()).toBe(false)
    expect(statuses).toEqual(['pending', 'saving', 'saved'])
  })

  test('flush saves immediately and cancels the debounce', async () => {
    const { clock, server, saver } = setup()
    saver.update({ title: 'Now' })
    const flushed = saver.flush()
    expect(server.calls).toHaveLength(1)
    expect(clock.pendingTimers()).toBe(0)
    server.calls[0].resolve(page(4))
    await flushed
    expect(saver.state.status).toBe('saved')
    // Nothing queued: another flush does not send a request.
    await saver.flush()
    expect(server.calls).toHaveLength(1)
  })

  test('single flight: edits during a save go out after it, with the version it returned', async () => {
    const { clock, server, saver } = setup()
    saver.update({ title: 'one' })
    void saver.flush()
    saver.update({ title: 'two' })
    saver.update({ content: [{ type: 'heading' }] })
    clock.advance(800) // the debounce fires while the first request is still out
    expect(server.calls).toHaveLength(1)
    expect(saver.hasUnsavedChanges()).toBe(true)

    server.calls[0].resolve(page(4))
    await tick()
    expect(server.calls).toHaveLength(2)
    expect(server.calls[1].version).toBe(4)
    expect(server.calls[1].patch).toEqual({ title: 'two', content: [{ type: 'heading' }] })

    server.calls[1].resolve(page(5))
    await tick()
    expect(saver.version).toBe(5)
    expect(saver.state.status).toBe('saved')
  })

  test('a flush during a save resolves only after the queued edits are saved too', async () => {
    const { server, saver } = setup()
    saver.update({ title: 'one' })
    void saver.flush()
    saver.update({ title: 'two' })
    let done = false
    void saver.flush().then(() => {
      done = true
    })
    server.calls[0].resolve(page(4))
    await tick()
    expect(done).toBe(false)
    server.calls[1].resolve(page(5))
    await tick()
    expect(done).toBe(true)
  })

  test('a conflict keeps local edits, stops autosaving, and overwrite re-sends them on the current version', async () => {
    const { clock, server, saver } = setup()
    saver.update({ title: 'mine' })
    void saver.flush()
    server.calls[0].reject(new Conflict(9))
    await tick()
    expect(saver.state.status).toBe('conflict')
    expect(saver.state.conflict?.currentVersion).toBe(9)
    expect(saver.hasUnsavedChanges()).toBe(true)

    // Further edits are kept but not sent while the user decides.
    saver.update({ content: [{ type: 'paragraph' }] })
    clock.advance(5000)
    await saver.flush()
    expect(server.calls).toHaveLength(1)

    void saver.overwrite({ title: 'mine', icon: null })
    expect(server.calls).toHaveLength(2)
    expect(server.calls[1].version).toBe(9)
    expect(server.calls[1].patch).toEqual({ title: 'mine', icon: null, content: [{ type: 'paragraph' }] })
    server.calls[1].resolve(page(10))
    await tick()
    expect(saver.state.status).toBe('saved')
    expect(saver.version).toBe(10)
  })

  test('a conflict the local copy can rebase onto (e.g. a move) retries on the new version without a conflict', async () => {
    const seen: Page[] = []
    const { server, saver, statuses } = setup(3, (current) => {
      seen.push(current)
      return true
    })
    saver.update({ title: 'mine' })
    void saver.flush()
    server.calls[0].reject(new Conflict(4, page(4)))
    await tick()
    expect(seen.map((item) => item.version)).toEqual([4])
    expect(server.calls).toHaveLength(2)
    expect(server.calls[1].version).toBe(4)
    expect(server.calls[1].patch).toEqual({ title: 'mine' })

    server.calls[1].resolve(page(5))
    await tick()
    expect(saver.state.status).toBe('saved')
    expect(statuses).not.toContain('conflict')
  })

  test('a conflict is reported when the server change touches edited fields', async () => {
    const { server, saver } = setup(3, () => false)
    saver.update({ title: 'mine' })
    void saver.flush()
    server.calls[0].reject(new Conflict(4, page(4)))
    await tick()
    expect(saver.state.status).toBe('conflict')
    expect(server.calls).toHaveLength(1)
  })

  test('reload (discard) drops local edits and continues from the server version', async () => {
    const { server, saver } = setup()
    saver.update({ title: 'mine' })
    void saver.flush()
    server.calls[0].reject(new Conflict(9))
    await tick()

    saver.discard(9)
    expect(saver.state.status).toBe('saved')
    expect(saver.hasUnsavedChanges()).toBe(false)
    saver.update({ title: 'after reload' })
    void saver.flush()
    expect(server.calls[1].version).toBe(9)
    expect(server.calls[1].patch).toEqual({ title: 'after reload' })
  })

  test('a failed save keeps the edits (merged under newer ones) and the next flush retries', async () => {
    const { server, saver } = setup()
    saver.update({ title: 'a', icon: '🙂' })
    void saver.flush()
    saver.update({ title: 'b' })
    server.calls[0].reject(new Error('offline'))
    await tick()
    expect(saver.state.status).toBe('error')
    expect(saver.hasUnsavedChanges()).toBe(true)

    void saver.flush()
    expect(server.calls[1].patch).toEqual({ title: 'b', icon: '🙂' })
    expect(server.calls[1].version).toBe(3)
  })

  test('adoptVersion only moves forward; dispose drops queued edits', async () => {
    const { clock, server, saver } = setup(3)
    saver.adoptVersion(2)
    expect(saver.version).toBe(3)
    saver.adoptVersion(6)
    expect(saver.version).toBe(6)

    saver.update({ title: 'gone' })
    saver.dispose()
    clock.advance(1000)
    await saver.flush()
    expect(server.calls).toHaveLength(0)
    expect(saver.hasUnsavedChanges()).toBe(false)
  })
})
