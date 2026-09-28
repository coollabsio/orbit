import { useState, type DragEvent } from 'react'
import type { Task } from '@/features/tasks/api/models'
import { nestZoneAt, planNestDrop, type NestPlan, type NestZone } from './nestDrop'
import type { TaskTree } from './taskTree'

/** Spread on a list row or board card. A `pass` hover or drop is left to the group zone around the row. */
export type NestRowProps = {
  'data-nest'?: NestZone
  onDragOver: (event: DragEvent<HTMLElement>) => void
  onDragLeave: (event: DragEvent<HTMLElement>) => void
  onDrop: (event: DragEvent<HTMLElement>) => void
}

type NestAt = { id: string; zone: NestZone }

/**
 * Drop onto a row or card (spec §7.4): its middle nests the dragged task under it; in a nested list the edges of a
 * sub-issue make the task its sibling. Everything else bubbles to `useGroupDrop`. `onTakeOver` clears the group
 * insertion line while a row owns the hover.
 */
export function useNestDrop({ tasks, tree, manual, dragId, onTakeOver, onNest, endDrag }: {
  tasks: Task[]
  /** The nested list's tree; null in a flat list and on the board (every edge passes). */
  tree: TaskTree | null
  manual: boolean
  dragId: string | null
  onTakeOver: () => void
  onNest: (plan: Extract<NestPlan, { kind: 'parent' }>, dragId: string) => void
  endDrag: () => void
}) {
  // keyed by the drag: a hover left over from a cancelled drag never shows on the next one
  const [hover, setHover] = useState<(NestAt & { dragId: string }) | null>(null)
  const nestAt: NestAt | null = hover && hover.dragId === dragId ? { id: hover.id, zone: hover.zone } : null

  const planAt = (event: DragEvent<HTMLElement>, target: Task) => {
    const zone = nestZoneAt(event.currentTarget.getBoundingClientRect(), event.clientY)
    const plan: NestPlan = dragId ? planNestDrop({ tasks, tree, dragId, target, zone, manual }) : { kind: 'pass' }
    return { zone, plan }
  }

  const rowProps = (target: Task): NestRowProps => ({
    'data-nest': nestAt?.id === target.id ? nestAt.zone : undefined,
    onDragOver: (event) => {
      const { zone, plan } = planAt(event, target)
      if (plan.kind === 'pass') {
        if (nestAt) setHover(null)
        return
      }
      event.preventDefault()
      event.stopPropagation()
      onTakeOver()
      if (plan.kind === 'invalid') {
        event.dataTransfer.dropEffect = 'none'
        if (nestAt) setHover(null)
        return
      }
      event.dataTransfer.dropEffect = 'move'
      if (dragId && (nestAt?.id !== target.id || nestAt.zone !== zone)) setHover({ id: target.id, zone, dragId })
    },
    onDragLeave: (event) => {
      if (!event.currentTarget.contains(event.relatedTarget as Node | null) && nestAt?.id === target.id) setHover(null)
    },
    onDrop: (event) => {
      const { plan } = planAt(event, target)
      if (plan.kind === 'pass') return // bubbles to the group zone
      event.preventDefault()
      event.stopPropagation()
      setHover(null)
      if (plan.kind === 'parent' && dragId) onNest(plan, dragId)
      endDrag()
    },
  })

  return { nestAt, rowProps }
}
