import type { WorkspaceRecord } from '@/api/generated/types.gen'

export function selectedWorkspaceId(
  workspaces: WorkspaceRecord[],
  search: string,
  preference: string | null,
): string | null {
  const routeId = new URLSearchParams(search).get('workspace')
  return [routeId, preference].find((id) => workspaces.some((workspace) => workspace.id === id))
    ?? workspaces[0]?.id
    ?? null
}

export function switchWorkspaceHref(pathname: string, search: string, workspaceId: string): string {
  const params = new URLSearchParams(search)
  params.set('workspace', workspaceId)
  return `${pathname}?${params}`
}
