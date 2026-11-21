import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SettingsCard } from '@/components/common/SettingsCard'
import { useSaveGithubAppClient, useStartGithubManifest, useGithubWorkspaceSettings } from '@/features/tasks/api/github'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { loadFailed } from '@/lib/connection'

export function GithubPage() {
  const { workspace } = useWorkspace()
  const query = useGithubWorkspaceSettings(workspace.id)
  const register = useStartGithubManifest(workspace.id)
  const resetRegistration = register.reset
  const [organization, setOrganization] = useState('')

  useEffect(() => {
    const clearOldError = () => resetRegistration()
    window.addEventListener('pageshow', clearOldError)
    return () => window.removeEventListener('pageshow', clearOldError)
  }, [resetRegistration])

  const beginRegistration = async () => {
    let data
    try { data = await register.mutateAsync(organization.trim()) } catch { return }
    const form = document.createElement('form')
    form.method = 'POST'
    form.action = data.action
    const input = document.createElement('input')
    input.type = 'hidden'
    input.name = 'manifest'
    input.value = JSON.stringify(data.manifest)
    form.append(input)
    document.body.append(form)
    form.submit()
  }

  if (loadFailed(query)) return <SettingsCard title="GitHub">GitHub settings are not available.</SettingsCard>
  if (query.data === undefined) return <SettingsCard title="GitHub">Loading GitHub settings.</SettingsCard>
  const settings = query.data

  return <SettingsCard title="GitHub" description={`Manage the GitHub App for this workspace. Connect repositories to projects in each project's settings. ${settings.app_slug ? "After installation, open a project's settings to select a repository and issue label." : 'Register a GitHub App from Orbit. GitHub will return you to this workspace.'}`}>
    {!settings.app_slug ? (
      <div className="flex flex-col items-start gap-2">
        {settings.can_manage ? <>
          <div className="grid w-full gap-1.5">
            <Label htmlFor="github-organization">GitHub organization (optional)</Label>
            <Input id="github-organization" value={organization} onChange={(event) => setOrganization(event.target.value)} placeholder="Leave blank for a personal App" />
          </div>
          {!settings.key_configured ? <p role="alert" className="text-xs text-destructive">Orbit is not ready to register a GitHub App yet. Ask the Orbit administrator to set the app key.</p> : null}
          <Button disabled={!settings.key_configured || register.isPending} onClick={() => void beginRegistration()}>Register GitHub App</Button>
          {register.isError ? <p role="alert" className="text-xs text-destructive">{register.error instanceof Error ? register.error.message : 'App registration could not start.'}</p> : null}
        </> : <p className="text-sm text-muted-foreground">Ask a workspace owner or admin to register the App.</p>}
      </div>
    ) : <div className="flex flex-col items-start gap-3">
      {settings.needs_recreate ? <div role="alert" className="flex w-full flex-col items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2.5">
        <p className="text-sm text-foreground">This GitHub App is from an older Orbit version. Upgrade it to review pull requests in Orbit.</p>
        {settings.can_manage ? <>
          <UpgradeGithubApp workspaceId={workspace.id} keyConfigured={settings.key_configured} />
          <p className="text-xs text-muted-foreground">You can also re-create the App. You must then install it again and connect each project to its repository again.</p>
          <Button variant="outline" disabled={!settings.key_configured || register.isPending} onClick={() => void beginRegistration()}>Re-create GitHub App</Button>
          {register.isError ? <p className="text-xs text-destructive">{register.error instanceof Error ? register.error.message : 'App registration could not start.'}</p> : null}
        </> : <p className="text-xs text-muted-foreground">Ask a workspace owner or admin to upgrade it.</p>}
      </div> : null}
      <p className="text-sm text-foreground">App: {settings.app_slug}</p>
      {settings.can_manage && settings.install_url ? <Button variant="outline" onClick={() => window.location.assign(settings.install_url!)}>Install or change repositories</Button> : null}
      <div className="w-full">
        <p className="mb-2 text-sm font-medium">Installed repositories</p>
        {settings.repositories.length > 0 ? (
          <ul aria-label="Installed repositories" className="grid gap-2">
            {settings.repositories.map((item) => <li key={`${item.installation_id}:${item.repository}`} className="rounded-md border px-3 py-2 text-sm">{item.repository}</li>)}
          </ul>
        ) : <p className="text-sm text-muted-foreground">No repositories installed yet.</p>}
      </div>
    </div>}
  </SettingsCard>
}

const PERMISSIONS = 'Contents: read and write, Pull requests: read and write, Issues: read, Checks: read, Commit statuses: read, Metadata: read'
const EVENTS = 'Issues, Pull request, Pull request review, Pull request review comment, Pull request review thread, Check suite, Check run, Status'

/** Adds the OAuth client to an app from before PR review, so its installations and project connections stay. */
function UpgradeGithubApp({ workspaceId, keyConfigured }: { workspaceId: string; keyConfigured: boolean }) {
  const save = useSaveGithubAppClient(workspaceId)
  const [clientId, setClientId] = useState('')
  const [clientSecret, setClientSecret] = useState('')
  const callbackUrl = `${window.location.origin}/api/v1/integrations/github/user/callback`
  return <form className="grid w-full gap-2" onSubmit={(event) => { event.preventDefault(); save.mutate({ client_id: clientId.trim(), client_secret: clientSecret.trim() }) }}>
    <ol className="list-decimal space-y-1 pl-5 text-sm text-foreground">
      <li>On GitHub, open the App's settings.</li>
      <li>Add this callback URL: <code className="break-all rounded bg-muted px-1 text-xs">{callbackUrl}</code></li>
      <li>Under Permissions, set {PERMISSIONS}.</li>
      <li>Subscribe to these events: {EVENTS}.</li>
      <li>Generate a new client secret. Paste it and the Client ID here.</li>
      <li>On each installation, accept the new permissions.</li>
    </ol>
    <div className="grid gap-1.5">
      <Label htmlFor="github-client-id">Client ID</Label>
      <Input id="github-client-id" value={clientId} onChange={(event) => setClientId(event.target.value)} autoComplete="off" />
    </div>
    <div className="grid gap-1.5">
      <Label htmlFor="github-client-secret">Client secret</Label>
      <Input id="github-client-secret" type="password" value={clientSecret} onChange={(event) => setClientSecret(event.target.value)} autoComplete="off" />
    </div>
    <div><Button type="submit" disabled={!keyConfigured || save.isPending || !clientId.trim() || !clientSecret.trim()}>Upgrade GitHub App</Button></div>
    {save.isError ? <p className="text-xs text-destructive">{save.error instanceof Error ? save.error.message : 'The App could not be upgraded.'}</p> : null}
  </form>
}
