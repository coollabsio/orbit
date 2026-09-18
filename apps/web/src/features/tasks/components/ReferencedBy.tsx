import { Link2, MessageText } from 'reicon-react'
import type { Task } from '../api/models'

interface ReferencedByProps {
  task: Task
  onOpen: (taskId: string) => void
}

/**
 * Derived backlinks: every live task or comment whose rich text mentions this
 * task. The server supplies the source's identifier and title, so no lookup.
 */
export function ReferencedBy({ task, onOpen }: ReferencedByProps) {
  if (task.referencedBy.length === 0) return null
  return (
    <div className="tasks-side-group">
      <h4 className="tasks-side-heading">Referenced by</h4>
      {task.referencedBy.map((reference) => {
        const comment = reference.sourceType === 'comment'
        const label = comment ? `Comment on ${reference.sourceTaskIdentifier}` : reference.sourceTaskIdentifier
        const Icon = comment ? MessageText : Link2
        return (
          <button
            key={`${reference.sourceType}:${reference.sourceId}`}
            type="button"
            className="button button-ghost tasks-side-prop tasks-side-link"
            aria-label={label}
            title={`${label} · ${reference.sourceTaskTitle}`}
            onClick={() => onOpen(reference.sourceTaskId)}
          >
            <Icon size={14} aria-hidden="true" />
            <span className="tasks-side-link-id">{label}</span>
            <span className="tasks-side-link-title truncate">{reference.sourceTaskTitle}</span>
          </button>
        )
      })}
    </div>
  )
}
