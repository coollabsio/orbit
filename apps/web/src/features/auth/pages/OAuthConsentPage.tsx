import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Navigate, useSearchParams } from 'react-router'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { LoadingScreen } from '@/components/common/LoadingScreen'
import { AccessPicker, ProjectPicker, type ScopePreset, type ScopeResource } from '@/components/common/ScopeAccess'
import { AuthBrand, AuthCard } from '@/features/auth/components/AuthForm'
import { currentUserQueryOptions } from '@/features/auth/api'
import { createApiClient } from '@/api/client'
import { oauthRequest, type OAuthConsent } from '@/features/auth/oauth'

// Anonymous consent sessions must not trigger the app-wide /login redirect.
const consentClient = createApiClient()

type Scope = 'tasks:write' | 'docs:read' | 'docs:write' | 'chat:read'

// Task read access comes with every grant, and chat has no write scope.
const RESOURCES: ScopeResource<Scope>[] = [
  { name: 'Tasks', hint: 'In the selected projects', write: 'tasks:write' },
  { name: 'Docs', hint: 'Including private docs', read: 'docs:read', write: 'docs:write' },
  { name: 'Chat', hint: 'Including private channels and direct messages', read: 'chat:read' },
]

const PRESETS: ScopePreset<Scope>[] = [
  { id: 'tasks-read', label: 'Tasks read-only', scopes: [] },
  { id: 'read-all', label: 'Read everything', scopes: ['docs:read', 'chat:read'] },
  { id: 'all', label: 'Full access', scopes: ['tasks:write', 'docs:read', 'docs:write', 'chat:read'] },
]

export function OAuthConsentPage() {
  const [params] = useSearchParams()
  const request = params.get('request') ?? ''
  const user = useQuery(currentUserQueryOptions(consentClient))
  const [remote, setRemote] = useState(false)
  const [callback, setCallback] = useState<{ url: string; approved: boolean } | null>(null)
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'failed'>('idle')
  const [workspace, setWorkspace] = useState('')
  const [selected, setSelected] = useState<string[]>([])
  const [scopes, setScopes] = useState<Scope[]>([])
  const consent = useQuery({
    queryKey: ['oauth-consent', request, user.data?.id],
    queryFn: () => oauthRequest<OAuthConsent>(`/api/v1/oauth/consent/${encodeURIComponent(request)}`),
    enabled: Boolean(user.data) && /^[A-Za-z0-9_-]{43}$/.test(request),
    retry: false,
    refetchOnWindowFocus: false,
  })
  const approve = useMutation({
    mutationFn: async (approved: boolean) => {
      const result = await oauthRequest<{ redirect_uri: string }>(`/api/v1/oauth/consent/${encodeURIComponent(request)}`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ csrf_token: consent.data?.csrf_token, approved, workspace_id: workspace || null, project_ids: selected, scopes }),
      })
      if (remote) setCallback({ url: result.redirect_uri, approved })
      else window.location.assign(result.redirect_uri)
    },
  })
  async function copyCallback() {
    if (!callback) return
    try {
      await navigator.clipboard.writeText(callback.url)
      setCopyState('copied')
    } catch {
      setCopyState('failed')
    }
  }
  if (callback) return <AuthCard><div className="grid gap-5">
    <AuthBrand />
    <h1 className="text-xl font-semibold">{callback.approved ? 'Complete the connection' : 'Connection denied'}</h1>
    <p className="text-sm text-muted-foreground">Paste the full callback URL into the terminal where you started sign-in with --no-browser.</p>
    {callback.approved ? <p className="text-sm text-muted-foreground">The authorization code expires after one minute. If it expires, restart sign-in from your client.</p> : null}
    <Label htmlFor="callback-url">Callback URL</Label>
    <textarea id="callback-url" readOnly value={callback.url} rows={4} className="w-full rounded-md border border-input bg-background p-3 font-mono text-xs break-all" onFocus={(event) => event.currentTarget.select()} />
    <Button onClick={() => void copyCallback()}>Copy callback URL</Button>
    <p role="status" className="text-sm text-muted-foreground">{copyState === 'copied' ? 'Callback URL copied.' : copyState === 'failed' ? 'Could not copy. Select the URL above and copy it manually.' : 'Keep this page open until the terminal confirms the result.'}</p>
  </div></AuthCard>
  if (user.isPending) return <LoadingScreen />
  if (user.data === null) return <Navigate to={`/login?return_to=${encodeURIComponent(`/oauth/consent?request=${request}`)}`} replace />
  const workspaceProjects = consent.data?.projects.filter((project) => project.workspace_id === workspace) ?? []
  const workspaces = [...new Map(consent.data?.projects.map((project) => [project.workspace_id, project.workspace_name])).entries()]
  return <AuthCard><div className="grid gap-5">
    <AuthBrand />
    <h1 className="text-xl font-semibold">Connect to Orbit</h1>
    {user.isError || consent.isError || !/^[A-Za-z0-9_-]{43}$/.test(request) ? <p role="alert">This request is not available. Restart the connection from your MCP client.</p> : consent.data ? <>
      <p><strong>{consent.data.client_name}</strong> wants to connect to the workspace and projects you select.</p>
      <p className="text-sm text-muted-foreground">Signed in as {user.data?.display_name}.</p>
      <Label>Workspace<Select value={workspace} onValueChange={(value) => { setWorkspace(value ?? ''); setSelected([]) }}>
        <SelectTrigger><SelectValue placeholder="Select a workspace">{workspaces.find(([id]) => id === workspace)?.[1] ?? 'Select a workspace'}</SelectValue></SelectTrigger>
        <SelectContent>{workspaces.map(([id, name]) => <SelectItem key={id} value={id}>{name}</SelectItem>)}</SelectContent>
      </Select></Label>
      <ProjectPicker id="consent-projects" projects={workspaceProjects} selected={selected} onChange={setSelected} empty={workspaces.length ? undefined : 'No active projects are available to your account.'} />
      <AccessPicker id="consent-access" resources={RESOURCES} presets={PRESETS} scopes={scopes} onChange={setScopes} />
      <div className="grid gap-2">
        <Label className="flex items-center gap-2">
          <Checkbox checked={remote} disabled={approve.isPending} onCheckedChange={(checked) => setRemote(Boolean(checked))} />
          Client runs on a remote server
        </Label>
        <p className="text-xs text-muted-foreground">Use --no-browser when you start sign-in in Claude or Codex. Orbit will show a callback URL to paste into that terminal.</p>
      </div>
      {approve.isError ? <p role="alert" className="text-sm text-destructive">{approve.error.message}</p> : null}
      <div className="flex gap-3">
        <Button variant="outline" disabled={approve.isPending} onClick={() => approve.mutate(false)}>Deny</Button>
        <Button disabled={!workspace || !selected.length || selected.length > 100 || approve.isPending} onClick={() => approve.mutate(true)}>{scopes.length ? 'Allow selected access' : 'Allow read access'}</Button>
      </div>
      <p className="text-xs text-muted-foreground">Allowing will redirect to <strong className="break-words font-medium text-foreground">{consent.data.redirect_to}</strong>.</p>
    </> : <p>Loading connection…</p>}
  </div></AuthCard>
}
