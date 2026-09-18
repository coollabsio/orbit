import { useState, type FormEvent } from 'react'
import { Copy, Key } from 'reicon-react'
import { confirmAction } from '../../components/ui/confirmAction'
import { Listbox } from '../../components/ui/Listbox'
import { relativeTime } from '../../lib/format'
import { useWorkspace } from '../workspaces/workspaceContext'
import { useProjects } from '../tasks/api/projects'
import { SettingsCard } from './SettingsCard'
import { useApiTokens, useCreateApiToken, useRevokeApiToken } from './api/apiTokens'

export function ApiTokensPage() {
  const { workspace } = useWorkspace()
  const canManage = workspace.role === 'owner' || workspace.role === 'admin'
  const tokens = useApiTokens(workspace.id, canManage)
  const projectsQuery = useProjects(workspace.id, canManage)
  const projects = projectsQuery.data ?? []
  const createToken = useCreateApiToken(workspace.id)
  const revokeToken = useRevokeApiToken(workspace.id)
  const [name, setName] = useState('')
  const [projectIds, setProjectIds] = useState<string[]>([])
  const projectNames = new Map(projects.map((project) => [project.id, project.name]))
  const [read, setRead] = useState(true)
  const [write, setWrite] = useState(true)
  const [issuedToken, setIssuedToken] = useState<string>()
  const [copied, setCopied] = useState(false)

  if (!canManage) return <SettingsCard title="API tokens"><p className="settings-notice">Only workspace owners and administrators can manage API tokens.</p></SettingsCard>

  async function submit(event: FormEvent) {
    event.preventDefault()
    const scopes = [...(read ? ['read' as const] : []), ...(write ? ['write' as const] : [])]
    if (!name.trim() || projectIds.length === 0 || scopes.length === 0) return
    const issued = await createToken.mutateAsync({ name: name.trim(), projectIds, scopes })
    setIssuedToken(issued.token)
    setName('')
    setCopied(false)
  }

  return <>
    <SettingsCard title="Create API token" description="Use API tokens for bots and other integrations. The secret is shown only once.">
      <form className="api-token-form settings-grid" onSubmit={(event) => void submit(event)}>
        <div className="settings-field"><label className="field-label" htmlFor="api-token-name">Name <span className="field-required">*</span></label><input id="api-token-name" className="input" value={name} maxLength={100} onChange={(event) => setName(event.target.value)} placeholder="Discord bot" /></div>
        <div className="settings-field"><label className="field-label" htmlFor="api-token-projects">Projects <span className="field-required">*</span></label><Listbox id="api-token-projects" aria-label="Projects" value={projectIds[0] ?? ''} selectedValues={projectIds} displayValue={projectIds.length === 0 ? undefined : projectIds.length === 1 ? projectNames.get(projectIds[0]) : `${projectIds.length} projects selected`} placeholder={projectsQuery.isPending ? 'Loading projects…' : 'Select projects…'} options={projects.map((project) => ({ value: project.id, label: project.name }))} disabled={projectsQuery.isPending || projects.length === 0} closeOnSelect={false} onChange={(projectId) => setProjectIds((current) => current.includes(projectId) ? current.filter((id) => id !== projectId) : [...current, projectId])} /></div>
        <fieldset className="api-token-scopes col-span-2"><legend className="field-label">Scopes</legend>
          <label><input type="checkbox" checked={read} onChange={(event) => setRead(event.target.checked)} /> Read</label>
          <label><input type="checkbox" checked={write} onChange={(event) => setWrite(event.target.checked)} /> Write</label>
        </fieldset>
        <button className="button button-primary" type="submit" disabled={createToken.isPending || !name.trim() || projectIds.length === 0 || (!read && !write)}>Create token</button>
        {createToken.isError ? <p className="text-danger" role="alert">The API token could not be created.</p> : null}
      </form>
      {issuedToken ? <div className="api-token-secret" role="status"><strong>Copy this token now. You cannot see it again.</strong><div><code>{issuedToken}</code><button className="button" type="button" onClick={() => { void navigator.clipboard.writeText(issuedToken); setCopied(true) }}><Copy size={16} /> {copied ? 'Copied' : 'Copy'}</button></div></div> : null}
    </SettingsCard>
    <SettingsCard title="Active API tokens" description="Revoke tokens that are no longer in use." flush>
      <div className="sessions-list">
        {tokens.isPending ? <div className="sessions-row">Loading API tokens…</div> : null}
        {tokens.isError ? <div className="sessions-row" role="alert">API tokens could not be loaded.</div> : null}
        {tokens.data?.length === 0 ? <div className="sessions-row">No active API tokens.</div> : null}
        {tokens.data?.map((token) => <div className="sessions-row" key={token.id}>
          <span className="sessions-device-icon"><Key size={18} /></span>
          <div className="sessions-text"><span className="sessions-device">{token.name}</span><span className="sessions-meta"><code>{token.token_prefix}…</code> · {token.project_ids.map((id) => projectNames.get(id) ?? 'Unavailable project').join(', ')} · created {relativeTime(token.created_at)}</span></div>
          <span className="api-token-scope-list">{token.scopes.map((scope) => <span className="badge" key={scope}>{scope}</span>)}</span>
          <button className="button button-ghost" type="button" disabled={revokeToken.isPending} onClick={async () => { if (await confirmAction({ title: `Revoke ${token.name}?`, description: 'The integration will lose access immediately.', confirmLabel: 'Revoke', danger: true })) revokeToken.mutate(token.id) }}>Revoke</button>
        </div>)}
      </div>
    </SettingsCard>
  </>
}
