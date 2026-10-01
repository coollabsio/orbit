import { useEffect, useState, type ReactNode } from 'react'
import { TaskPointerContext, TaskTargetContext, trackPointer, type TaskPointer, type TaskTargetState } from './taskTarget'

/** Holds what task commands act on for one tasks page: the selection, the pointer, and the open task. */
export function TaskTargetProvider({ openTaskId, children }: { openTaskId: string | null; children: ReactNode }) {
  const [selectedIds, setSelected] = useState<string[]>([])
  const [pointer] = useState<TaskPointer>(() => ({ hoveredId: null, x: -1, y: -1, focusedId: null }))
  const [order] = useState<TaskTargetState['order']>(() => ({ current: [] }))
  useEffect(() => trackPointer(pointer), [pointer])
  return (
    <TaskPointerContext value={pointer}>
      <TaskTargetContext value={{ selectedIds, setSelected, openTaskId, order }}>{children}</TaskTargetContext>
    </TaskPointerContext>
  )
}
