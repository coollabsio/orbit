import { useState, type FormEvent } from 'react'
import { Copy, Key } from 'lucide-react'
import { confirmAction } from '../../components/ui/confirmAction'
import { Listbox } from '../../components/ui/Listbox'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from 'cn'
import { relativeTime, shortDate } from '../../lib/format'
import { useWorkspace } from '../workspaces/workspaceContext'
import { useProjects } from '../tasks/api/projects'
import { SettingsCard } from './SettingsCard'
import { useApiTokens, useCreateApiToken, useRevokeApiToken } from './api/apiTokens'

const EXPIRATION_OPTIONS = [
  { value: 'never', label: 'Never expires' },
  { value: '7', label: '7 days' },
  { value: '30', label: '30 days' },
  { value: '90', label: '90 days' },
  { value: '365', label: '1 year' },
] as const

type ExpirationValue = typeof EXPIRATION_OPTIONS[number]['value']

const FIELD_LABEL = 'mb-1.5 h-4 gap-1 text-[13px] leading-4 font-medium text-muted-foreground'
const GRID = 'grid grid-cols-1 gap-4 min-[900px]:grid-cols-2'
const FIELD = 'w-full min-w-0'
const REQ = 'inline-block font-semibold text-primary'
const BADGE = 'inline-flex items-center gap-1 rounded-full bg-sidebar-accent px-2 py-0.5 text-[10px] leading-[14px] font-medium whitespace-nowrap text-muted-foreground'

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
  const [serviceAccount, setServiceAccount] = useState(false)
  const [expiration, setExpiration] = useState<ExpirationValue>('never')
  const [issuedToken, setIssuedToken] = useState<string>()
  const [copied, setCopied] = useState(false)

  if (!canManage) return <SettingsCard title="API tokens"><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage API tokens.</p></SettingsCard>

  async function submit(event: FormEvent) {
    event.preventDefault()
    const scopes = [...(read ? ['read' as const] : []), ...(write ? ['write' as const] : [])]
    if (!name.trim() || projectIds.length === 0 || scopes.length === 0) return
    const issued = await createToken.mutateAsync({ name: name.trim(), projectIds, scopes, serviceAccount, expiresInDays: expiration === 'never' ? null : Number(expiration) })
    setIssuedToken(issued.token)
    setName('')
    setCopied(false)
  }

  return <>
    <SettingsCard title="Create API token" description="Create a personal token, or select a non-login service account for an integration. The secret is shown only once.">
      <form className={cn('w-full', GRID)} onSubmit={(event) => void submit(event)}>
        <div className={FIELD}><Label className={FIELD_LABEL} htmlFor="api-token-name">Name <span className={REQ}>*</span></Label><Input id="api-token-name" value={name} maxLength={100} onChange={(event) => setName(event.target.value)} placeholder="Discord bot" /></div>
        <div className={FIELD}><Label className={FIELD_LABEL} htmlFor="api-token-projects">Projects <span className={REQ}>*</span></Label><Listbox id="api-token-projects" aria-label="Projects" value={projectIds[0] ?? ''} selectedValues={projectIds} displayValue={projectIds.length === 0 ? undefined : projectIds.length === 1 ? projectNames.get(projectIds[0]) : `${projectIds.length} projects selected`} placeholder={projectsQuery.isPending ? 'Loading projects…' : 'Select projects…'} options={projects.map((project) => ({ value: project.id, label: project.name }))} disabled={projectsQuery.isPending || projects.length === 0} closeOnSelect={false} onChange={(projectId) => setProjectIds((current) => current.includes(projectId) ? current.filter((id) => id !== projectId) : [...current, projectId])} /></div>
        <div className={FIELD}><Label className={FIELD_LABEL} htmlFor="api-token-expiration">Expiration</Label><Listbox id="api-token-expiration" aria-label="Expiration" value={expiration} options={[...EXPIRATION_OPTIONS]} onChange={setExpiration} /></div>
        <fieldset className="flex gap-[18px] border-0 p-0"><legend className="mb-2 text-xs font-semibold text-muted-foreground">Scopes</legend>
          <label className="flex items-center gap-[7px] text-[13px]"><input type="checkbox" checked={read} onChange={(event) => setRead(event.target.checked)} /> Read</label>
          <label className="flex items-center gap-[7px] text-[13px]"><input type="checkbox" checked={write} onChange={(event) => setWrite(event.target.checked)} /> Write</label>
        </fieldset>
        <label className="flex cursor-pointer items-start gap-2.5 rounded-lg border border-input bg-muted p-3 transition-colors hover:border-border data-[selected=true]:border-primary data-[selected=true]:bg-primary/10 min-[900px]:col-span-2" data-selected={serviceAccount}>
          <input type="checkbox" className="mt-px size-4 shrink-0" checked={serviceAccount} onChange={(event) => setServiceAccount(event.target.checked)} />
          <span className="grid min-w-0 gap-[3px]"><strong className="text-[13px] leading-4 font-semibold text-foreground">Use a service account</strong><span className="text-xs leading-4 text-muted-foreground">For Discord and other unattended integrations. Tokens with the same name reuse this identity.</span></span>
        </label>
        <Button className="justify-self-start" type="submit" disabled={createToken.isPending || !name.trim() || projectIds.length === 0 || (!read && !write)}>Create token</Button>
        {createToken.isError ? <p className="text-destructive" role="alert">The API token could not be created.</p> : null}
      </form>
      {issuedToken ? <div className="mt-[18px] grid gap-2.5 rounded-lg border border-primary bg-primary/10 p-3 text-xs" role="status"><strong>Copy this token now. You cannot see it again.</strong><div className="flex items-center gap-2"><code className="min-w-0 flex-1 [overflow-wrap:anywhere] select-all">{issuedToken}</code><Button variant="outline" type="button" onClick={() => { void navigator.clipboard.writeText(issuedToken); setCopied(true) }}><Copy className="size-4" /> {copied ? 'Copied' : 'Copy'}</Button></div></div> : null}
    </SettingsCard>
    <SettingsCard title="Active API tokens" description="Revoke tokens that are no longer in use." flush>
      <div className="flex flex-col divide-y divide-border">
        {tokens.isPending ? <div className="flex items-center gap-3 px-4 py-3">Loading API tokens…</div> : null}
        {tokens.isError ? <div className="flex items-center gap-3 px-4 py-3" role="alert">API tokens could not be loaded.</div> : null}
        {tokens.data?.length === 0 ? <div className="flex items-center gap-3 px-4 py-3">No active API tokens.</div> : null}
        {tokens.data?.map((token) => <div className="flex items-center gap-3 px-4 py-3" key={token.id}>
          <span className="inline-flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground"><Key className="size-4.5" /></span>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5"><span className="flex items-center gap-2 text-[13px] font-medium text-foreground">{token.name}</span><span className="text-xs text-muted-foreground/70">{token.service_account_name ? `Service account: ${token.service_account_name}` : 'Personal token'} · <code>{token.token_prefix}…</code> · {token.project_ids.map((id) => projectNames.get(id) ?? 'Unavailable project').join(', ')} · {token.expires_at ? `expires ${shortDate(token.expires_at)}` : 'never expires'} · created {relativeTime(token.created_at)}</span></div>
          <span className="flex gap-1">{token.scopes.map((scope) => <span className={BADGE} key={scope}>{scope}</span>)}</span>
          <Button variant="ghost" type="button" disabled={revokeToken.isPending} onClick={async () => { if (await confirmAction({ title: `Revoke ${token.name}?`, description: 'The integration will lose access immediately.', confirmLabel: 'Revoke', danger: true })) revokeToken.mutate(token.id) }}>Revoke</Button>
        </div>)}
      </div>
    </SettingsCard>
  </>
}
