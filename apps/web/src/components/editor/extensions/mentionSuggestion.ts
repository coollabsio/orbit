import type { Editor, Range } from '@tiptap/core'
import { ReactRenderer } from '@tiptap/react'
import type { SuggestionOptions, SuggestionProps } from '@tiptap/suggestion'
import type { TaskRecord } from '../../../api/generated/types.gen'
import type { User } from '../../../features/tasks/api/models'
import { MentionList } from '../MentionList'
import { buildMentionItems, createTaskLookup } from './mentionItems'
import type { MentionItem } from './mentionItems'

export {
  MAX_TASK_SUGGESTIONS,
  MAX_USER_SUGGESTIONS,
  SUGGESTION_DEBOUNCE_MS,
  buildMentionItems,
  createTaskLookup,
} from './mentionItems'
export type { MentionItem } from './mentionItems'

export interface MentionContext {
  workspaceId: string
  members: () => User[]
  /** Called with the full record when an issue is picked, before it is inserted. */
  onPickTask?: (task: TaskRecord) => void
}

/**
 * The node a picked item becomes. Attributes are exactly the server allowlist:
 * `mention {id, label}` and `taskMention {id, identifier}` — nothing else.
 */
export function mentionNodeFor(item: MentionItem) {
  return item.kind === 'user'
    ? { type: 'mention', attrs: { id: item.id, label: item.label } }
    : { type: 'taskMention', attrs: { id: item.id, identifier: item.identifier } }
}

/**
 * Replaces the `@query` range with the picked node plus a trailing space. It
 * replaces TipTap's default command, which always inserts a `mention` and tags
 * it with a `mentionSuggestionChar` attribute the server would reject.
 */
function insertMention({ editor, range, props }: { editor: Editor; range: Range; props: MentionItem }) {
  const nodeAfter = editor.view.state.selection.$to.nodeAfter
  const to = nodeAfter?.text?.startsWith(' ') ? range.to + 1 : range.to
  editor
    .chain()
    .focus()
    .insertContentAt({ from: range.from, to }, [mentionNodeFor(props), { type: 'text', text: ' ' }])
    .run()
}

type MenuProps = SuggestionProps<MentionItem, MentionItem>

/**
 * A SINGLE unified `@`. People appear immediately from the member list; issues
 * join below them once the debounced fts5 search answers. While a search is in
 * flight the previous issues stay visible, so the list does not flicker on
 * every keystroke.
 */
export function createMentionSuggestion(
  context: MentionContext,
): Omit<SuggestionOptions<MentionItem, MentionItem>, 'editor'> {
  const lookup = createTaskLookup(context.workspaceId)
  return {
    char: '@',
    allowSpaces: false,
    command: insertMention,
    // The plugin only tracks the query; the menu below owns item assembly so
    // people never wait on the network.
    items: () => [],
    render: () => {
      let renderer: ReactRenderer | undefined
      let unmount: (() => void) | undefined
      let query = ''
      let tasks: TaskRecord[] = []
      let items: MentionItem[] = []
      let activeIndex = 0
      let insert: ((item: MentionItem) => void) | undefined
      const command = (item: MentionItem) => {
        if (item.kind === 'task') {
          const task = tasks.find((candidate) => candidate.id === item.id)
          if (task) context.onPickTask?.(task)
        }
        insert?.(item)
      }

      const paint = () => {
        renderer?.updateProps({
          items,
          activeIndex,
          onSelect: command,
          onHover: (index: number) => {
            activeIndex = index
            paint()
          },
        })
      }

      const rebuild = () => {
        items = buildMentionItems({ query, members: context.members(), tasks })
        activeIndex = Math.min(activeIndex, Math.max(items.length - 1, 0))
        paint()
      }

      const search = (next: string) => {
        if (next !== query) activeIndex = 0
        query = next
        if (next.trim() === '') tasks = []
        rebuild()
        void lookup(next).then((found) => {
          // A superseded lookup resolves to [] — only the current query may repaint.
          if (!renderer || next !== query) return
          tasks = found
          rebuild()
        })
      }

      return {
        onStart: (props: MenuProps) => {
          insert = props.command
          renderer = new ReactRenderer(MentionList, {
            props: { items: [], activeIndex: 0, onSelect: () => {}, onHover: () => {} },
            editor: props.editor,
            className: 'editor-mention-layer',
          })
          unmount = props.mount(renderer.element)
          search(props.query)
        },
        onUpdate: (props: MenuProps) => {
          insert = props.command
          if (props.query !== query) search(props.query)
        },
        onKeyDown: ({ event }) => {
          if (event.key === 'Escape') return true
          if (items.length === 0) return false
          if (event.key === 'ArrowDown') {
            activeIndex = (activeIndex + 1) % items.length
            paint()
            return true
          }
          if (event.key === 'ArrowUp') {
            activeIndex = (activeIndex - 1 + items.length) % items.length
            paint()
            return true
          }
          if (event.key === 'Enter' || event.key === 'Tab') {
            command(items[activeIndex])
            return true
          }
          return false
        },
        onExit: () => {
          unmount?.()
          unmount = undefined
          renderer?.destroy()
          renderer = undefined
          void lookup('')
        },
      }
    },
  }
}
