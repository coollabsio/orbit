import { createExtension, type ExtensionOptions } from '@blocknote/core'
import type { EditorState } from '@tiptap/pm/state'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'
import { relativePositionToAbsolutePosition, yCursorPluginKey, ySyncPluginKey } from 'y-prosemirror'
import type { Awareness } from 'y-protocols/awareness'
import * as Y from 'yjs'
import { inkOn } from './cursor'
import { initials } from './presence'

interface BlockUser {
  id: string
  name: string
  color: string
}

/** The marker text of a block: the first person's initials, then "+n" for the others. */
export function presenceLabel(users: readonly BlockUser[]): string {
  const first = initials(users[0].name) || '?'
  return users.length > 1 ? `${first} +${users.length - 1}` : first
}

/** Start of the innermost block (`blockContainer`) around `pos`, or null outside any block. */
function blockAt(state: EditorState, pos: number): number | null {
  const resolved = state.doc.resolve(pos)
  for (let depth = resolved.depth; depth > 0; depth -= 1) {
    if (resolved.node(depth).type.name === 'blockContainer') return resolved.before(depth)
  }
  return null
}

/** One node decoration for each block that holds someone else's caret (several tabs of one person count once). */
function presenceDecorations(state: EditorState, awareness: Awareness, self: string | undefined): DecorationSet {
  const ystate = ySyncPluginKey.getState(state)
  // no positions to map yet, or a version preview is shown
  if (!ystate?.binding || ystate.binding.mapping.size === 0 || ystate.snapshot != null || ystate.prevSnapshot != null) return DecorationSet.empty
  const blocks = new Map<number, BlockUser[]>()
  awareness.getStates().forEach((value, clientId) => {
    const user = value.user as Partial<BlockUser> | undefined
    if (clientId === awareness.clientID || !value.cursor || typeof user?.id !== 'string' || user.id === self) return
    const head = relativePositionToAbsolutePosition(ystate.doc, ystate.type, Y.createRelativePositionFromJSON(value.cursor.head), ystate.binding.mapping)
    if (head === null) return
    const block = blockAt(state, Math.min(head, state.doc.content.size))
    if (block === null) return
    const users = blocks.get(block) ?? []
    if (!users.some((other) => other.id === user.id)) users.push({ id: user.id, name: user.name || 'Someone', color: user.color || '#737373' })
    blocks.set(block, users)
  })
  const decorations = [...blocks].map(([pos, users]) =>
    Decoration.node(pos, pos + state.doc.nodeAt(pos)!.nodeSize, {
      'data-collab-presence': presenceLabel(users),
      style: `--collab-color: ${users[0].color}; --collab-ink: ${inkOn(users[0].color)}`,
    }),
  )
  return DecorationSet.create(state.doc, decorations)
}

const presenceKey = new PluginKey<DecorationSet>('collabBlockPresence')

/**
 * Notion-style presence: a small marker in the right margin of each block someone else is in, in their cursor color.
 * Recomputed when y-prosemirror's cursor plugin sees awareness change or a remote edit, like the carets themselves.
 */
export const collabBlockPresence = createExtension(({ options }: ExtensionOptions<{ awareness: Awareness; /** The signed-in user: their other tabs get no marker. */ selfId: string | undefined }>) => ({
  key: 'collabBlockPresence',
  prosemirrorPlugins: [
    new Plugin<DecorationSet>({
      key: presenceKey,
      state: {
        init: (_, state) => presenceDecorations(state, options.awareness, options.selfId),
        apply(tr, previous, _old, state) {
          if (ySyncPluginKey.getState(state)?.isChangeOrigin || tr.getMeta(yCursorPluginKey)?.awarenessUpdated) {
            return presenceDecorations(state, options.awareness, options.selfId)
          }
          return previous.map(tr.mapping, tr.doc)
        },
      },
      props: { decorations: (state) => presenceKey.getState(state) },
    }),
  ],
}))
