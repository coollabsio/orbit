import type { OptionalTaskProperty, WorkspaceRecord } from './generated/types.gen'

/** The built-in task properties a workspace may hide (Settings, Task fields), shared by the web and mobile apps. */
export const OPTIONAL_TASK_PROPERTIES: { property: OptionalTaskProperty; label: string; help: string }[] = [
  { property: 'milestone', label: 'Milestone', help: 'Group a project’s tasks into milestones. Off: tasks, filters, grouping and project settings do not show milestones.' },
  { property: 'cycle', label: 'Cycle', help: 'Plan tasks in time-boxed cycles. Off: tasks, filters, grouping and project settings do not show cycles.' },
  { property: 'estimate', label: 'Estimate', help: 'Size tasks with points. Off: tasks, filters and ordering do not show estimates.' },
]

/** Whether the workspace shows the property in its task UI. Hiding keeps every task's value. */
export function showsTaskProperty(workspace: Pick<WorkspaceRecord, 'hidden_task_properties'> | null | undefined, property: OptionalTaskProperty): boolean {
  return !workspace?.hidden_task_properties?.includes(property)
}

/** The hidden properties after turning `property` on or off. */
export function withTaskProperty(workspace: Pick<WorkspaceRecord, 'hidden_task_properties'>, property: OptionalTaskProperty, shown: boolean): OptionalTaskProperty[] {
  const hidden = (workspace.hidden_task_properties ?? []).filter((item) => item !== property)
  return shown ? hidden : [...hidden, property]
}

/** Whether a property key from display, filter, grouping or ordering state is one the workspace hides. */
export function hidesTaskKey(workspace: Pick<WorkspaceRecord, 'hidden_task_properties'> | null | undefined, key: string): boolean {
  return OPTIONAL_TASK_PROPERTIES.some(({ property }) => property === key && !showsTaskProperty(workspace, property))
}
