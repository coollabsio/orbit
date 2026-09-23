import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SettingsCard } from '@/components/common/SettingsCard'
import { useStartGithubManifest, useGithubWorkspaceSettings } from '@/features/tasks/api/github'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

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

  if (query.isPending) return <SettingsCard title="GitHub">Loading GitHub settings.</SettingsCard>
  if (query.isError) return <SettingsCard title="GitHub">GitHub settings are not available.</SettingsCard>
  const settings = query.data

  return <SettingsCard title="GitHub" description="Manage the GitHub App for this workspace. Connect repositories to projects in each project's settings.">
    {!settings.app_slug ? (
      <div className="flex flex-col items-start gap-2">
        <p className="text-sm text-muted-foreground">Register a GitHub App from Orbit. GitHub will return you to this workspace.</p>
        {settings.can_manage ? <>
          <div className="grid w-full gap-1.5">
            <Label htmlFor="github-organization">GitHub organization (optional)</Label>
            <Input id="github-organization" value={organization} onChange={(event) => setOrganization(event.target.value)} placeholder="Leave blank for a personal App" />
          </div>
          {!settings.key_configured ? <p role="alert" className="text-xs text-destructive">The server needs an App key before you can register an App.</p> : null}
          <Button disabled={!settings.key_configured || register.isPending} onClick={() => void beginRegistration()}>Register GitHub App</Button>
          {register.isError ? <p role="alert" className="text-xs text-destructive">{register.error instanceof Error ? register.error.message : 'App registration could not start.'}</p> : null}
        </> : <p className="text-sm text-muted-foreground">Ask a workspace owner or admin to register the App.</p>}
      </div>
    ) : <div className="flex flex-col items-start gap-3">
      <p className="text-sm text-foreground">App: {settings.app_slug}</p>
      {settings.can_manage && settings.install_url ? <Button variant="outline" onClick={() => window.location.assign(settings.install_url!)}>Install or change repositories</Button> : null}
      <div className="w-full">
        <p className="mb-2 text-sm font-medium">Installed repositories</p>
        {settings.repositories.length > 0 ? (
          <ul aria-label="Installed repositories" className="grid gap-2">
            {settings.repositories.map((item) => <li key={`${item.installation_id}:${item.repository}`} className="rounded-md border border-border px-3 py-2 text-sm">{item.repository}</li>)}
          </ul>
        ) : <p className="text-sm text-muted-foreground">No repositories installed yet.</p>}
      </div>
      <p className="text-sm text-muted-foreground">After installation, open a project's settings to select a repository and issue label.</p>
    </div>}
  </SettingsCard>
}
