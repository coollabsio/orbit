export interface WorkspaceEvent {
  version: 1
  kind: 'workspace.changed' | 'resync_required'
  sequence: string
  workspaces_changed?: boolean
  profile_changed?: boolean
  /** Every change since the last event is a saved-view write; missing means false. */
  views_only?: boolean
}

export function parseEvent(raw: string): WorkspaceEvent | null {
  try {
    const value = JSON.parse(raw)
    if (value?.version !== 1 || !['workspace.changed', 'resync_required'].includes(value.kind)
      || typeof value.sequence !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value.sequence)
      || (value.workspaces_changed !== undefined && typeof value.workspaces_changed !== 'boolean')
      || (value.profile_changed !== undefined && typeof value.profile_changed !== 'boolean')
      || (value.views_only !== undefined && typeof value.views_only !== 'boolean')) return null
    return value
  } catch { return null }
}
