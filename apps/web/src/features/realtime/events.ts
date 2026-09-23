export interface WorkspaceEvent {
  version: 1
  kind: 'workspace.changed' | 'resync_required'
  sequence: string
  workspaces_changed?: boolean
  profile_changed?: boolean
}

export function parseEvent(raw: string): WorkspaceEvent | null {
  try {
    const value = JSON.parse(raw)
    if (value?.version !== 1 || !['workspace.changed', 'resync_required'].includes(value.kind)
      || typeof value.sequence !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value.sequence)
      || (value.workspaces_changed !== undefined && typeof value.workspaces_changed !== 'boolean')
      || (value.profile_changed !== undefined && typeof value.profile_changed !== 'boolean')) return null
    return value
  } catch { return null }
}
