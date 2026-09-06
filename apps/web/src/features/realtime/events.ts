export interface WorkspaceEvent {
  version: 1
  kind: 'workspace.changed' | 'resync_required'
  sequence: string
}

export function parseEvent(raw: string): WorkspaceEvent | null {
  try {
    const value = JSON.parse(raw)
    if (value?.version !== 1 || !['workspace.changed', 'resync_required'].includes(value.kind)
      || typeof value.sequence !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value.sequence)) return null
    return value
  } catch { return null }
}
