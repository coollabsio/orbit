import type { ProjectBody } from '@orbit/api/generated/types.gen'

/** A new project from its name: a key of up to five letters and digits, with the smallest free number after it
 * when an existing project already has that key (`GENER` → `GENER2`), and violet. */
export function projectDraft(name: string, takenKeys: readonly string[] = []): ProjectBody {
  const base = name.replace(/[^a-z0-9]/gi, '').slice(0, 5).toUpperCase() || 'PROJ'
  const taken = new Set(takenKeys.map((key) => key.toUpperCase()))
  let key = base
  for (let suffix = 2; taken.has(key); suffix += 1) key = `${base}${suffix}`
  return { name, key, color: '#8b5cf6' }
}

export function projectSettingsPath(projectId: string | null): string | null {
  return projectId ? `/tasks/projects/${projectId}/settings` : null
}

export function projectSettingsLabel(): string {
  return 'Project settings'
}
