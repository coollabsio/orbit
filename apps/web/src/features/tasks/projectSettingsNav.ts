import type { ComponentType } from 'react'
import type { OptionalTaskProperty, WorkspaceRecord } from '@orbit/api/generated/types.gen'
import { showsTaskProperty } from '@orbit/api/taskProperties'
import { Calendar, Category, Danger, DocumentText, Flag, Hierarchy, Link2, Lock, Setting2 as Settings, Tag, TaskSquare } from 'reicon-react'

export type ProjectSettingsSection = 'general' | 'access' | 'statuses' | 'labels' | 'fields' | 'sub-issues' | 'milestones' | 'cycles' | 'docs' | 'github' | 'danger-zone'

export interface ProjectSettingsGroup {
  label: string
  /** `property`: the built-in task property the page sets up; the page goes when the workspace hides it. */
  items: { section: ProjectSettingsSection; label: string; icon: ComponentType<{ className?: string }>; property?: OptionalTaskProperty }[]
}

/** The project settings pages, grouped as the sidebar shows them. */
const PROJECT_SETTINGS_GROUPS: ProjectSettingsGroup[] = [
  {
    label: 'Project',
    items: [
      { section: 'general', label: 'General', icon: Settings },
      { section: 'access', label: 'Access', icon: Lock },
    ],
  },
  {
    label: 'Workflow',
    items: [
      { section: 'statuses', label: 'Statuses', icon: Category },
      { section: 'labels', label: 'Labels', icon: Tag },
      { section: 'fields', label: 'Fields', icon: TaskSquare },
      { section: 'sub-issues', label: 'Sub-issues', icon: Hierarchy },
    ],
  },
  {
    label: 'Planning',
    items: [
      { section: 'milestones', label: 'Milestones', icon: Flag, property: 'milestone' },
      { section: 'cycles', label: 'Cycles', icon: Calendar, property: 'cycle' },
    ],
  },
  {
    label: 'Integrations',
    items: [
      { section: 'docs', label: 'Docs', icon: DocumentText },
      { section: 'github', label: 'GitHub', icon: Link2 },
    ],
  },
  {
    label: 'Advanced',
    items: [{ section: 'danger-zone', label: 'Danger zone', icon: Danger }],
  },
]

type TaskPropertyScope = Pick<WorkspaceRecord, 'hidden_task_properties'> | null | undefined

/** The settings pages the workspace shows: without those of a built-in property it hides (Settings, Task fields). */
export function projectSettingsGroups(workspace: TaskPropertyScope): ProjectSettingsGroup[] {
  return PROJECT_SETTINGS_GROUPS
    .map((group) => ({ ...group, items: group.items.filter((item) => !item.property || showsTaskProperty(workspace, item.property)) }))
    .filter((group) => group.items.length > 0)
}

/** The page of a `:section` route segment; an unknown or hidden one is `null`. */
export function projectSettingsItem(section: string, workspace: TaskPropertyScope) {
  return projectSettingsGroups(workspace).flatMap((group) => group.items).find((item) => item.section === section) ?? null
}

export function projectSettingsPath(projectId: string, section: ProjectSettingsSection): string {
  return `/tasks/projects/${projectId}/settings/${section}`
}

/** The project whose settings the path opens. */
export function projectSettingsProjectOf(pathname: string): string | null {
  return /^\/tasks\/projects\/([^/]+)\/settings(\/|$)/.exec(pathname)?.[1] ?? null
}
