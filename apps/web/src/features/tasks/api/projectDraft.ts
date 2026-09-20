import type { ProjectBody } from '@/api/generated/types.gen'

export function projectDraft(name: string): ProjectBody {
  return {
    name,
    key: name.replace(/[^a-z0-9]/gi, '').slice(0, 5).toUpperCase() || 'PROJ',
    color: '#8b5cf6',
  }
}

export function projectSettingsPath(projectId: string | null): string | null {
  return projectId ? `/tasks/projects/${projectId}/settings` : null
}

export function projectSettingsLabel(): string {
  return 'Project settings'
}
