import { use } from 'react'
import { Link } from 'react-router'
import { TaskStatusIcon } from '../workspace/TaskStatusIcon'
import { TaskChipContext, type ResolvedChip } from './taskChipContext'

/**
 * `identifier` is the fallback cached in the document. `resolved` is the live
 * value from the batch resolve; when it is missing the target is gone (or not
 * loaded yet) and the chip degrades to the cached identifier alone.
 */
export function TaskChip({ identifier, resolved }: { identifier: string; resolved?: ResolvedChip }) {
  const body = (
    <>
      {resolved ? (
        <TaskStatusIcon
          status={{ category: resolved.statusCategory, color: resolved.statusColor ?? 'var(--text-faint)' }}
          size={12}
        />
      ) : null}
      <span className="editor-chip-id">{identifier}</span>
      {resolved ? <span className="editor-chip-title truncate">{resolved.title}</span> : null}
    </>
  )
  const attributes = {
    className: 'editor-chip',
    'data-resolved': resolved ? 'true' : undefined,
    'data-cancelled': resolved?.statusCategory === 'cancelled' ? 'true' : undefined,
    title: resolved ? `${identifier} · ${resolved.title}` : identifier,
  }

  return resolved?.taskId ? (
    // A chip inside a click-to-edit description navigates; it must not also open the editor.
    <Link {...attributes} to={`/tasks/${resolved.taskId}`} onClick={(event) => event.stopPropagation()}>
      {body}
    </Link>
  ) : (
    <span {...attributes}>{body}</span>
  )
}

/** The static-render mapping for a `taskMention` node: live values come from context. */
export function TaskMentionChip({ node }: { node: { attrs?: Record<string, unknown> } }) {
  const chips = use(TaskChipContext)
  const identifier = String(node.attrs?.identifier ?? '')
  return <TaskChip identifier={identifier} resolved={chips?.(identifier)} />
}
