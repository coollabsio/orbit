import { toast } from 'sonner'
import { useState, type FormEvent } from 'react'
import { Copy, Key } from 'reicon-react'
import { confirmAction } from '@/components/common/confirmAction'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Field, FieldContent, FieldDescription, FieldLabel, FieldTitle } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { relativeTime, shortDate } from '@/lib/format'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useProjects } from '@/features/tasks/api/projects'
import { SettingsCard } from '@/components/common/SettingsCard'
import { AccessPicker, ProjectPicker, type ScopePreset, type ScopeResource } from '@/components/common/ScopeAccess'
import { FieldGrid, RequiredMark, RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { useApiTokens, useCreateApiToken, useRevokeApiToken } from '@/features/settings/api/apiTokens'
import type { ApiTokenScope } from '@orbit/api/generated/types.gen'

const EXPIRATION_OPTIONS = [
  { value: 'never', label: 'Never expires' },
  { value: '7', label: '7 days' },
  { value: '30', label: '30 days' },
  { value: '90', label: '90 days' },
  { value: '365', label: '1 year' },
] as const

type ExpirationValue = typeof EXPIRATION_OPTIONS[number]['value']

const PRESETS: ScopePreset<ApiTokenScope>[] = [
  { id: 'tasks-read', label: 'Tasks read-only', scopes: ['tasks:read'] },
  { id: 'tasks-write', label: 'Tasks read and write', scopes: ['tasks:read', 'tasks:write'] },
  { id: 'read-all', label: 'Read everything', scopes: ['tasks:read', 'docs:read', 'chat:read'] },
  { id: 'all', label: 'Full access', scopes: ['tasks:read', 'tasks:write', 'docs:read', 'docs:write', 'chat:read'] },
]

function tokenResources(serviceAccount: boolean): ScopeResource<ApiTokenScope>[] {
  return [
    { name: 'Tasks', hint: 'In the selected projects. Write includes delete.', read: 'tasks:read', write: 'tasks:write' },
    { name: 'Docs', hint: 'Including your private docs', read: 'docs:read', write: 'docs:write', disabled: serviceAccount },
    { name: 'Chat', hint: 'Including your private channels and DMs. MCP only.', read: 'chat:read', disabled: serviceAccount },
  ]
}


function isExpired(expiresAt?: string | null) {
  return expiresAt != null && new Date(expiresAt).getTime() <= Date.now()
}

export function ApiTokensPage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('api_tokens.manage')
  const tokens = useApiTokens(workspace.id, canManage)
  const projectsQuery = useProjects(workspace.id, canManage)
  const projects = projectsQuery.data ?? []
  const createToken = useCreateApiToken(workspace.id)
  const revokeToken = useRevokeApiToken(workspace.id)
  const [name, setName] = useState('')
  const [projectIds, setProjectIds] = useState<string[]>([])
  const projectNames = new Map(projects.map((project) => [project.id, project.name]))
  const [scopes, setScopes] = useState<ApiTokenScope[]>(['tasks:read', 'tasks:write'])
  const [serviceAccount, setServiceAccount] = useState(false)
  const [expiration, setExpiration] = useState<ExpirationValue>('never')
  const [issuedToken, setIssuedToken] = useState<string>()
  const [copied, setCopied] = useState(false)

  // Projects only limit task access, so a token without a task scope needs none.
  const taskScope = scopes.some((scope) => scope.startsWith('tasks:'))
  const ready = name.trim() !== '' && scopes.length > 0 && (!taskScope || projectIds.length > 0)

  if (!canManage) return <SettingsCard title="API tokens"><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage API tokens.</p></SettingsCard>

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (!ready) return
    const issued = await createToken.mutateAsync({ name: name.trim(), projectIds: taskScope ? projectIds : [], scopes, serviceAccount, expiresInDays: expiration === 'never' ? null : Number(expiration) })
    setIssuedToken(issued.token)
    setName('')
    setCopied(false)
  }

  return <>
    <SettingsCard title="Create API token" description="Create a personal token, or select a non-login service account for an integration. The secret is shown only once.">
      <form onSubmit={(event) => void submit(event)}>
        <FieldGrid>
        <Field><FieldLabel htmlFor="api-token-name">Name <RequiredMark /></FieldLabel><Input id="api-token-name" value={name} maxLength={100} onChange={(event) => setName(event.target.value)} placeholder="Discord bot" /></Field>
        <Field>
          <FieldLabel htmlFor="api-token-expiration">Expiration</FieldLabel>
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
        {taskScope ? <div className="min-[900px]:col-span-2">
          <ProjectPicker id="api-token-projects" projects={projects} selected={projectIds} onChange={setProjectIds} empty={projectsQuery.isPending ? 'Loading projects…' : projects.length ? undefined : 'This workspace has no active projects.'} />
        </div> : null}
        <div className="min-[900px]:col-span-2">
          <AccessPicker id="api-token-access" resources={tokenResources(serviceAccount)} presets={PRESETS} scopes={scopes} onChange={setScopes} />
        </div>
        {/* a choice card: the whole card toggles the checkbox and tints while checked */}
        <FieldLabel className="min-[900px]:col-span-2">
          <Field orientation="horizontal">
            <Checkbox checked={serviceAccount} disabled={scopes.some((scope) => scope.startsWith('docs:') || scope === 'chat:read')} onCheckedChange={setServiceAccount} />
            <FieldContent>
              <FieldTitle>Use a service account</FieldTitle>
              <FieldDescription>For Discord and other unattended integrations. Tokens with the same name reuse this identity.</FieldDescription>
            </FieldContent>
          </Field>
        </FieldLabel>
        <Button className="justify-self-start" type="submit" disabled={createToken.isPending || !ready}>Create token</Button>
        {createToken.isError ? <p className="text-destructive" role="alert">The API token could not be created.</p> : null}
        </FieldGrid>
      </form>
      {issuedToken ? <div className="mt-[18px] grid gap-2.5 rounded-lg border border-primary bg-primary/10 p-3 text-xs" role="status"><strong>Copy this token now. You cannot see it again.</strong><div className="flex items-center gap-2"><code className="min-w-0 flex-1 [overflow-wrap:anywhere] select-all">{issuedToken}</code><Button variant="outline" type="button" onClick={() => { void navigator.clipboard.writeText(issuedToken).then(() => { setCopied(true); toast('Token copied') }, () => toast.error('Could not copy the token.')) }}><Copy className="size-4" /> {copied ? 'Copied' : 'Copy'}</Button></div></div> : null}
    </SettingsCard>
    <SettingsCard title="Active API tokens" description="Revoke tokens that are no longer in use." flush>
      <div className="flex flex-col divide-y">
        {tokens.isPending ? <SettingsRow>Loading API tokens…</SettingsRow> : null}
        {tokens.isError ? <SettingsRow role="alert">API tokens could not be loaded.</SettingsRow> : null}
        {tokens.data?.length === 0 ? <SettingsRow>No active API tokens.</SettingsRow> : null}
        {tokens.data?.map((token) => <SettingsRow className="flex-nowrap" key={token.id}>
          <RowIcon><Key /></RowIcon>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5"><span className="flex items-center gap-2 text-[13px] font-medium text-foreground">{token.name}{isExpired(token.expires_at) ? <Badge variant="destructive">Expired</Badge> : null}</span><span className="text-xs text-muted-foreground/70">{token.service_account_name ? `Service account: ${token.service_account_name}` : 'Personal token'} · <code>{token.token_prefix}…</code> · {token.project_ids.length ? `${token.project_ids.map((id) => projectNames.get(id) ?? 'Unavailable project').join(', ')} · ` : null}{token.expires_at ? `${isExpired(token.expires_at) ? 'expired' : 'expires'} ${shortDate(token.expires_at)}` : 'never expires'} · created {relativeTime(token.created_at)}</span></div>
          <span className="flex gap-1">{token.scopes.map((scope) => <Badge variant="secondary" key={scope}>{scope}</Badge>)}</span>
          <Button variant="ghost" type="button" disabled={revokeToken.isPending} onClick={async () => { if (await confirmAction({ title: `Revoke ${token.name}?`, description: 'The integration will lose access immediately.', confirmLabel: 'Revoke', danger: true })) revokeToken.mutate(token.id) }}>Revoke</Button>
        </SettingsRow>)}
      </div>
    </SettingsCard>
  </>
}
