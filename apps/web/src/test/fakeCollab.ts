// A stand-in for y-websocket + the Orbit collab server in tests: a real Y.Doc and Awareness, a provider whose socket
// events the test drives, and a server-like initializer (page JSON -> Y fragment, never empty).
import { BlockNoteEditor } from '@blocknote/core'
import { blocksToYXmlFragment } from '@blocknote/core/yjs'
import { Awareness, applyAwarenessUpdate, encodeAwarenessUpdate } from 'y-protocols/awareness'
import * as Y from 'yjs'
import { COLLAB_FRAGMENT, type CollabConnect, type CollabTarget } from '@/features/docs/collab/connection'
import type { CollabConnection, CollabProviderLike } from '@/features/docs/collab/session'
import { toEditorContent } from '@/features/docs/editor/content'
import { EDITOR_BLOCK_TYPES, pageEditorSchema } from '@/features/docs/editor/schema'

type Listener = (...args: never[]) => void

let headless: BlockNoteEditor | null = null
/** Enough for JSON -> Y conversions; never mounted. */
function converter() {
  headless ??= BlockNoteEditor.create({ schema: pageEditorSchema, _tiptapOptions: { injectCSS: false } }) as unknown as BlockNoteEditor
  return headless
}

/** Writes `blocks` into the fragment the way the server's converter does (empty content → one paragraph). */
export function writeBlocks(fragment: Y.XmlFragment, blocks: unknown[]) {
  const safe = toEditorContent(blocks, EDITOR_BLOCK_TYPES) ?? [{ type: 'paragraph' }]
  blocksToYXmlFragment(converter() as never, safe as never, fragment)
}

export class FakeProvider implements CollabProviderLike {
  awareness: Awareness
  synced = false
  wsconnected = false
  destroyed = false
  connectCalls = 0
  readonly doc: Y.Doc
  private readonly listeners = new Map<string, Set<Listener>>()

  constructor(doc: Y.Doc) {
    this.doc = doc
    this.awareness = new Awareness(doc)
  }

  on(event: string, listener: Listener) {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set())
    this.listeners.get(event)!.add(listener)
  }

  off(event: string, listener: Listener) {
    this.listeners.get(event)?.delete(listener)
  }

  emit(event: string, ...args: unknown[]) {
    for (const listener of [...(this.listeners.get(event) ?? [])]) (listener as (...values: unknown[]) => void)(...args)
  }

  connect() {
    this.connectCalls += 1
  }

  destroy() {
    this.destroyed = true
    this.awareness.destroy()
  }

  /** The socket opened (y-websocket: `status: connected`). */
  open() {
    this.emit('status', { status: 'connecting' })
    this.wsconnected = true
    this.emit('status', { status: 'connected' })
  }

  /** The server's SyncStep2 arrived. */
  sync() {
    this.synced = true
    this.emit('sync', true)
  }

  /** Open + sync. */
  connectAndSync() {
    this.open()
    this.sync()
  }

  /** The socket closed (y-websocket order: connection-close, sync false, status disconnected, closed if terminal). */
  drop(code = 1006) {
    this.emit('connection-close', { code, reason: '' })
    if (this.wsconnected) {
      this.wsconnected = false
      this.synced = false
      this.emit('sync', false)
      this.emit('status', { status: 'disconnected' })
    }
    if (code >= 4400 && code < 4500) this.emit('closed', { code, reason: '' })
  }

  /** A handshake refused before the upgrade (HTTP 401/403/404 reach the browser as 1006). */
  refuse() {
    this.emit('status', { status: 'connecting' })
    this.emit('connection-close', { code: 1006, reason: '' })
  }

  /** A change from the server (another editor, a restore): applied with the provider as origin, like y-websocket. */
  remote(change: (fragment: Y.XmlFragment) => void) {
    Y.transact(this.doc, () => change(this.doc.getXmlFragment(COLLAB_FRAGMENT)), this)
  }

  /** Server-side replacement of the whole document (restore, Notion import, REST content PATCH). */
  replace(blocks: unknown[]) {
    this.remote((fragment) => writeBlocks(fragment, blocks))
  }

  /** Someone else joins with a server-stamped user (their own awareness client id). */
  addPeer(user: { id: string; name: string; color: string }, cursor: unknown = null): number {
    const peerDoc = new Y.Doc()
    const peer = new Awareness(peerDoc)
    peer.setLocalState({ user, cursor })
    applyAwarenessUpdate(this.awareness, encodeAwarenessUpdate(peer, [peerDoc.clientID]), 'remote')
    peer.destroy()
    peerDoc.destroy()
    return peerDoc.clientID
  }
}

export interface FakeConnection extends CollabConnection {
  provider: FakeProvider
  target: CollabTarget
}

export interface FakeCollab {
  connect: CollabConnect
  connections: FakeConnection[]
  last: () => FakeConnection
}

/**
 * `content(target)` is what the server holds for the page. With `autoSync` (default) every connection opens and
 * syncs on the next tick, like a healthy server.
 */
export function fakeCollab(options: { content?: (target: CollabTarget) => unknown[]; autoSync?: boolean } = {}): FakeCollab {
  const connections: FakeConnection[] = []
  const connect: CollabConnect = async (target) => {
    const doc = new Y.Doc()
    const fragment = doc.getXmlFragment(COLLAB_FRAGMENT)
    // The server's initial state arrives as a remote update.
    const seed = new Y.Doc()
    writeBlocks(seed.getXmlFragment(COLLAB_FRAGMENT), options.content?.(target) ?? [])
    const provider = new FakeProvider(doc)
    const connection: FakeConnection = { doc, provider, fragment, target }
    connections.push(connection)
    if (options.autoSync ?? true) {
      setTimeout(() => {
        if (provider.destroyed) return
        provider.open()
        Y.applyUpdate(doc, Y.encodeStateAsUpdate(seed), provider)
        provider.sync()
      }, 0)
    } else {
      // Tests call `provider.connectAndSync()` themselves; the state is there already.
      Y.applyUpdate(doc, Y.encodeStateAsUpdate(seed), provider)
    }
    seed.destroy()
    return connection
  }
  return { connect, connections, last: () => connections.at(-1)! }
}

/** Plain text of a collaborative document (all text nodes in order). */
export function fragmentText(fragment: Y.XmlFragment): string {
  const out: string[] = []
  const walk = (node: Y.XmlElement | Y.XmlFragment | Y.XmlText) => {
    if (node instanceof Y.XmlText) out.push(node.toString().replace(/<[^>]+>/g, ''))
    else for (const child of node.toArray()) walk(child as Y.XmlElement | Y.XmlText)
  }
  walk(fragment)
  return out.join(' ')
}
