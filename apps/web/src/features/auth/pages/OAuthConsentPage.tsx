import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Navigate, useSearchParams } from 'react-router'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { LoadingScreen } from '@/components/common/LoadingScreen'
import { AuthBrand, AuthCard } from '@/features/auth/components/AuthForm'
import { currentUserQueryOptions } from '@/features/auth/api'
import { createApiClient } from '@/api/client'
import { oauthRequest, type OAuthConsent } from '@/features/auth/oauth'

// Anonymous consent sessions must not trigger the app-wide /login redirect.
const consentClient = createApiClient()

export function OAuthConsentPage() {
  const [params] = useSearchParams()
  const request = params.get('request') ?? ''
  const user = useQuery(currentUserQueryOptions(consentClient))
  const [remote, setRemote] = useState(false)
  const [callback, setCallback] = useState<{ url: string; approved: boolean } | null>(null)
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'failed'>('idle')
  const [workspace, setWorkspace] = useState('')
  const [selected, setSelected] = useState<string[]>([])
  const [scopes, setScopes] = useState<string[]>([])
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
  const workspaces = [...new Map(consent.data?.projects.map((project) => [project.workspace_id, project.workspace_name])).entries()]
  return <AuthCard><div className="grid gap-5">
    <AuthBrand />
    <h1 className="text-xl font-semibold">Connect to Orbit</h1>
    {user.isError || consent.isError || !/^[A-Za-z0-9_-]{43}$/.test(request) ? <p role="alert">This request is not available. Restart the connection from your MCP client.</p> : consent.data ? <>
      <p><strong>{consent.data.client_name}</strong> wants to connect to the workspace and projects you select.</p>
      <p className="rounded-md border border-input p-3 text-sm">If you allow access, Orbit sends the access code to <strong className="break-all">{consent.data.redirect_to}</strong>. Allow it only if you started this connection and you trust that address. The client chose its own name, and Orbit did not verify it.</p>
      <p className="text-sm text-muted-foreground">Signed in as {user.data?.display_name}. Tasks are read-only unless you enable task writes below.</p>
      <Label>Workspace<Select value={workspace} onValueChange={(value) => { setWorkspace(value ?? ''); setSelected([]) }}>
        <SelectTrigger><SelectValue placeholder="Select a workspace">{workspaces.find(([id]) => id === workspace)?.[1] ?? 'Select a workspace'}</SelectValue></SelectTrigger>
        <SelectContent>{workspaces.map(([id, name]) => <SelectItem key={id} value={id}>{name}</SelectItem>)}</SelectContent>
      </Select></Label>
      <fieldset className="grid gap-3"><legend className="mb-3 text-sm font-medium">Projects</legend>
        {consent.data.projects.filter((project) => project.workspace_id === workspace).map((project) => <Label key={project.id} className="flex items-center gap-2">
          <Checkbox checked={selected.includes(project.id)} onCheckedChange={(checked) => setSelected((ids) => checked ? [...ids, project.id] : ids.filter((id) => id !== project.id))} />
          {project.name}
        </Label>)}
        {!workspaces.length ? <p className="text-sm text-muted-foreground">No active projects are available to your account.</p> : null}
      </fieldset>
      <fieldset className="grid gap-3"><legend className="mb-3 text-sm font-medium">Optional permissions</legend>
        {([['tasks:write', 'Create, change and delete tasks in selected projects'], ['docs:read', 'Read workspace docs visible to you, including private docs'], ['chat:read', 'Read chat visible to you, including private channels and direct messages']] as const).map(([scope, label]) => <Label key={scope} className="flex items-center gap-2">
          <Checkbox checked={scopes.includes(scope)} onCheckedChange={(checked) => setScopes((current) => checked ? [...current, scope] : current.filter((s) => s !== scope))} />{label}
        </Label>)}
      </fieldset>
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
    </> : <p>Loading connection…</p>}
  </div></AuthCard>
}
