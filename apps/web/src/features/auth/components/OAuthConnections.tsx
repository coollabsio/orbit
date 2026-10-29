import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { SettingsCard } from '@/components/common/SettingsCard'
import type { OAuthConnection } from '@orbit/api/mcp'
import { oauthRequest } from '@/features/auth/oauth'

export function OAuthConnections() {
  const queryClient = useQueryClient()
  const connections = useQuery({ queryKey: ['oauth-grants'], queryFn: () => oauthRequest<{ items: OAuthConnection[] }>('/api/v1/oauth/grants') })
  const disconnect = useMutation({
    mutationFn: (id: string) => oauthRequest<void>(`/api/v1/oauth/grants/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['oauth-grants'] }),
  })
  return <SettingsCard title="Connected apps" description="MCP clients with approved read-only task access. Disconnecting an app stops its access and refresh tokens.">
    {connections.isPending ? <p>Loading connections…</p> : null}
    {connections.isError || disconnect.isError ? <p role="alert">Connections could not be updated. Try again.</p> : null}
    {connections.data?.items.length === 0 ? <p className="text-sm text-muted-foreground">No connected apps.</p> : null}
    <div className="flex flex-col divide-y">{connections.data?.items.map((connection) => <div key={connection.id} className="flex items-center justify-between gap-4 py-3">
      <div><p className="text-sm font-medium">{connection.client_name}</p><p className="text-xs text-muted-foreground">{connection.workspace_name} · Read tasks · {connection.project_names.join(', ')}</p></div>
      <Button variant="outline" disabled={disconnect.isPending} onClick={() => disconnect.mutate(connection.id)}>Disconnect</Button>
    </div>)}</div>
  </SettingsCard>
}
