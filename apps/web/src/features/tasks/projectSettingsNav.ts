import type { ComponentType } from 'react'
import { Calendar, Category, Danger, DocumentText, Flag, Hierarchy, Link2, Lock, Setting2 as Settings, Tag } from 'reicon-react'

export type ProjectSettingsSection = 'general' | 'access' | 'statuses' | 'labels' | 'sub-issues' | 'milestones' | 'cycles' | 'docs' | 'github' | 'danger-zone'

export interface ProjectSettingsGroup {
  label: string
  items: { section: ProjectSettingsSection; label: string; icon: ComponentType<{ className?: string }> }[]
}

/** The project settings pages, grouped as the sidebar shows them. */
export const PROJECT_SETTINGS_GROUPS: ProjectSettingsGroup[] = [
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
      { section: 'sub-issues', label: 'Sub-issues', icon: Hierarchy },
    ],
  },
  {
    label: 'Planning',
    items: [
      { section: 'milestones', label: 'Milestones', icon: Flag },
      { section: 'cycles', label: 'Cycles', icon: Calendar },
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

const ITEMS = PROJECT_SETTINGS_GROUPS.flatMap((group) => group.items)

/** The page of a `:section` route segment; an unknown one is `null`. */
export function projectSettingsItem(section: string) {
  return ITEMS.find((item) => item.section === section) ?? null
}

export function projectSettingsPath(projectId: string, section: ProjectSettingsSection): string {
  return `/tasks/projects/${projectId}/settings/${section}`
}

/** The project whose settings the path opens. */
export function projectSettingsProjectOf(pathname: string): string | null {
  return /^\/tasks\/projects\/([^/]+)\/settings(\/|$)/.exec(pathname)?.[1] ?? null
}
