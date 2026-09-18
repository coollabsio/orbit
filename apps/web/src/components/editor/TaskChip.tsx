import { use } from 'react'
import { TaskChipContext } from './taskChipContext'

export function TaskChip({
  identifier,
  resolved,
}: {
  identifier: string
  resolved?: { title: string; statusCategory: string }
}) {
  return (
    <span
      className="editor-chip"
      data-resolved={resolved ? 'true' : undefined}
      data-cancelled={resolved?.statusCategory === 'cancelled' ? 'true' : undefined}
    >
      <span className="editor-chip-id">{identifier}</span>
      {resolved ? <span className="editor-chip-title truncate">{resolved.title}</span> : null}
    </span>
  )
}

/** The static-render mapping for a `taskMention` node: live values come from context. */
export function TaskMentionChip({ node }: { node: { attrs?: Record<string, unknown> } }) {
  const chips = use(TaskChipContext)
  const identifier = String(node.attrs?.identifier ?? '')
  return <TaskChip identifier={identifier} resolved={chips?.(identifier)} />
}
