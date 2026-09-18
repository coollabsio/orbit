import { Extension } from '@tiptap/core'
import type { EditorState, Transaction } from '@tiptap/pm/state'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import type { EditorView } from '@tiptap/pm/view'
import { IDENTIFIER_PATTERN } from '../document'
import type { TaskResolver } from './taskResolver'

const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
const WHOLE_IDENTIFIER = new RegExp(`^${IDENTIFIER_PATTERN.source}$`)

function taskSegment(url: string, origin: string): string | null {
  try {
    const parsed = new URL(url)
    if (parsed.origin !== origin) return null
    const [, section, segment] = parsed.pathname.split('/')
    return section === 'tasks' && segment ? segment : null
  } catch {
    return null
  }
}

/** A same-origin `/tasks/<uuid>` link. */
export function taskIdFromTaskUrl(url: string, origin: string): string | null {
  const segment = taskSegment(url, origin)
  return segment && UUID_V7.test(segment) ? segment : null
}

/** A same-origin `/tasks/<IDENTIFIER>` link. */
export function identifierFromTaskUrl(url: string, origin: string): string | null {
  const segment = taskSegment(url, origin)
  return segment && WHOLE_IDENTIFIER.test(segment) ? segment : null
}

/** `ORB-12` followed by a space or newline becomes a chip, matching Linear. */
export function createIdentifierInputRule() {
  return { find: new RegExp(`(?:^|\\s)(${IDENTIFIER_PATTERN.source})[\\s]$`) }
}

/** What a pasted string refers to, if anything: a bare identifier or a same-origin task link. */
export function taskReferenceInPaste(text: string, origin: string): { identifier?: string; taskId?: string } | null {
  const value = text.trim()
  if (WHOLE_IDENTIFIER.test(value)) return { identifier: value }
  const identifier = identifierFromTaskUrl(value, origin)
  if (identifier) return { identifier }
  const taskId = taskIdFromTaskUrl(value, origin)
  return taskId ? { taskId } : null
}

interface Pending {
  token: number
  from: number
  to: number
  text: string
}

type Meta = { add: Pending; mapThrough: boolean } | { remove: number }

const pluginKey = new PluginKey<Pending[]>('identifierPaste')

function inCode(state: EditorState, pos: number): boolean {
  const $pos = state.doc.resolve(pos)
  return Boolean($pos.parent.type.spec.code) || $pos.marks().some((mark) => mark.type.name === 'code')
}

export interface IdentifierPasteOptions {
  /** Resolution is injected so this extension stays free of React Query and the API client. */
  resolve: TaskResolver | null
}

/**
 * Turns a typed bare identifier (`ORB-12` then a space) or a pasted identifier
 * or task link into a `taskMention` chip. The text lands immediately; once the
 * resolver answers, the range — mapped through any edits made meanwhile — is
 * swapped for the chip, but only if it still holds exactly that text. Unknown
 * identifiers simply stay text. Nothing happens inside code.
 */
export const IdentifierPaste = Extension.create<IdentifierPasteOptions>({
  name: 'identifierPaste',

  addOptions() {
    return { resolve: null }
  },

  addProseMirrorPlugins() {
    const { resolve } = this.options
    if (!resolve) return []
    let nextToken = 0

    const settle = (view: EditorView, pending: Pending, request: ReturnType<TaskResolver>) => {
      void request.then((reference) => {
        if (view.isDestroyed) return
        const current = pluginKey.getState(view.state)?.find((entry) => entry.token === pending.token)
        const tr = view.state.tr.setMeta(pluginKey, { remove: pending.token } satisfies Meta)
        if (current && reference && view.state.doc.textBetween(current.from, current.to) === current.text) {
          tr.replaceWith(current.from, current.to, view.state.schema.nodes.taskMention.create(reference))
        }
        view.dispatch(tr)
      })
    }

    const track = (view: EditorView, tr: Transaction, pending: Pending, mapThrough: boolean, input: Parameters<TaskResolver>[0]) => {
      view.dispatch(tr.setMeta(pluginKey, { add: pending, mapThrough } satisfies Meta))
      settle(view, pending, resolve(input))
    }

    return [
      new Plugin<Pending[]>({
        key: pluginKey,
        state: {
          init: () => [],
          apply: (tr, pending) => {
            let next = pending
              .map((entry) => ({ ...entry, from: tr.mapping.map(entry.from, 1), to: tr.mapping.map(entry.to, -1) }))
              .filter((entry) => entry.from < entry.to)
            const meta = tr.getMeta(pluginKey) as Meta | undefined
            if (meta && 'remove' in meta) next = next.filter((entry) => entry.token !== meta.remove)
            if (meta && 'add' in meta) {
              const { add, mapThrough } = meta
              next = [
                ...next,
                mapThrough ? { ...add, from: tr.mapping.map(add.from, 1), to: tr.mapping.map(add.to, -1) } : add,
              ]
            }
            return next
          },
        },
        props: {
          handleTextInput: (view, from, to, text, deflt) => {
            if (!/^\s$/.test(text) || from !== to || inCode(view.state, from)) return false
            const $from = view.state.doc.resolve(from)
            const before = $from.parent.textBetween(0, $from.parentOffset, undefined, '￼')
            const match = `${before}${text}`.match(createIdentifierInputRule().find)
            if (!match) return false
            const identifier = match[1]
            const pending = { token: nextToken++, from: from - identifier.length, to: from, text: identifier }
            track(view, deflt(), pending, true, { identifier })
            return true
          },
          handlePaste: (view, _event, slice) => {
            const text = slice.content.textBetween(0, slice.content.size, '\n').trim()
            const reference = text ? taskReferenceInPaste(text, window.location.origin) : null
            if (!reference || inCode(view.state, view.state.selection.from)) return false
            const insert = view.state.tr.insertText(text)
            const end = insert.selection.from
            view.dispatch(insert.scrollIntoView())
            track(view, view.state.tr, { token: nextToken++, from: end - text.length, to: end, text }, false, reference)
            return true
          },
        },
      }),
    ]
  },
})
