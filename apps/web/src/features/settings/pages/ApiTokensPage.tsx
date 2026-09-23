import { useState, type FormEvent } from 'react'
import { Copy, Key } from 'reicon-react'
import { confirmAction } from '@/components/common/confirmAction'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { cn } from 'cn'
import { relativeTime, shortDate } from '@/lib/format'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useProjects } from '@/features/tasks/api/projects'
import { SettingsCard } from '@/components/common/SettingsCard'
import { useApiTokens, useCreateApiToken, useRevokeApiToken } from '@/features/settings/api/apiTokens'

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
// `gap-0` keeps the Field rows at the previous label/control spacing (the label owns its `mb-1.5`).
const FIELD = 'w-full min-w-0 gap-0'
const REQ = 'inline-block font-semibold text-primary'
const BADGE = 'h-auto rounded-full border-0 bg-sidebar-accent text-[10px] leading-[14px] text-muted-foreground'

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
        <Field className={FIELD}><FieldLabel className={FIELD_LABEL} htmlFor="api-token-name">Name <span className={REQ}>*</span></FieldLabel><Input id="api-token-name" value={name} maxLength={100} onChange={(event) => setName(event.target.value)} placeholder="Discord bot" /></Field>
        <Field className={FIELD}>
          <FieldLabel className={FIELD_LABEL} htmlFor="api-token-projects">Projects <span className={REQ}>*</span></FieldLabel>
          {/* Base UI's `multiple` replaces the old Listbox `selectedValues` + `closeOnSelect={false}` pair: the popup stays open while toggling projects. */}
          <Select multiple value={projectIds} disabled={projectsQuery.isPending || projects.length === 0} onValueChange={(value) => setProjectIds(value)}>
            <SelectTrigger id="api-token-projects" aria-label="Projects">
              <SelectValue>
                {(value: string[]) => value.length === 0
                  ? <span className="text-muted-foreground">{projectsQuery.isPending ? 'Loading projects…' : 'Select projects…'}</span>
                  : value.length === 1 ? projectNames.get(value[0]) : `${value.length} projects selected`}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              {projects.map((project) => <SelectItem key={project.id} value={project.id}>{project.name}</SelectItem>)}
            </SelectContent>
          </Select>
        </Field>
        <Field className={FIELD}>
          <FieldLabel className={FIELD_LABEL} htmlFor="api-token-expiration">Expiration</FieldLabel>
          {/* `items` lets Select.Value render the option label instead of the raw value. */}
          <Select items={EXPIRATION_OPTIONS} value={expiration} onValueChange={(value) => setExpiration(value as ExpirationValue)}>
            <SelectTrigger id="api-token-expiration" aria-label="Expiration">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {EXPIRATION_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </Field>
        <fieldset className="flex gap-[18px] border-0 p-0"><legend className="mb-2 text-xs font-semibold text-muted-foreground">Scopes</legend>
          <Label className="gap-[7px] text-[13px] leading-[inherit] font-normal"><Checkbox checked={read} onCheckedChange={setRead} /> Read</Label>
          <Label className="gap-[7px] text-[13px] leading-[inherit] font-normal"><Checkbox checked={write} onCheckedChange={setWrite} /> Write</Label>
        </fieldset>
        <Label className="cursor-pointer items-start gap-2.5 rounded-lg border border-input bg-muted p-3 transition-colors hover:border-border data-[selected=true]:border-primary data-[selected=true]:bg-primary/10 min-[900px]:col-span-2" data-selected={serviceAccount}>
          <Checkbox className="mt-px" checked={serviceAccount} onCheckedChange={setServiceAccount} />
          <span className="grid min-w-0 gap-[3px]"><strong className="text-[13px] leading-4 font-semibold text-foreground">Use a service account</strong><span className="text-xs leading-4 text-muted-foreground">For Discord and other unattended integrations. Tokens with the same name reuse this identity.</span></span>
        </Label>
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
          <span className="flex gap-1">{token.scopes.map((scope) => <Badge className={BADGE} key={scope}>{scope}</Badge>)}</span>
          <Button variant="ghost" type="button" disabled={revokeToken.isPending} onClick={async () => { if (await confirmAction({ title: `Revoke ${token.name}?`, description: 'The integration will lose access immediately.', confirmLabel: 'Revoke', danger: true })) revokeToken.mutate(token.id) }}>Revoke</Button>
        </div>)}
      </div>
    </SettingsCard>
  </>
}
