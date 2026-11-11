import { toast } from 'sonner'
import { useState, type FormEvent, type ReactNode } from 'react'
import { Copy, Key } from 'reicon-react'
import { confirmAction } from '@/components/common/confirmAction'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Field, FieldContent, FieldLabel, FieldTitle } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { relativeTime, shortDate } from '@/lib/format'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useProjects } from '@/features/tasks/api/projects'
import { SettingsCard } from '@/components/common/SettingsCard'
import { InfoTip } from '@/components/common/InfoTip'
import { AccessPicker, ProjectPicker, type ScopeResource } from '@/components/common/ScopeAccess'
import { FieldGrid, RequiredMark, RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { useApiTokens, useCreateApiToken, useRevokeApiToken } from '@/features/settings/api/apiTokens'
import type { ApiTokenScope } from '@orbit/api/generated/types.gen'
import { LoadingText } from '@/components/ui/spinner'

const EXPIRATION_OPTIONS = [
  { value: 'never', label: 'Never expires' },
  { value: '7', label: '7 days' },
  { value: '30', label: '30 days' },
  { value: '90', label: '90 days' },
  { value: '365', label: '1 year' },
] as const

type ExpirationValue = typeof EXPIRATION_OPTIONS[number]['value']

/** An export scope stands alone on a personal token, so it is a kind of token, not an access level. */
const TOKEN_KINDS = [
  { value: 'access', label: 'Tasks, docs and chat', description: 'For scripts, bots and AI clients.' },
  { value: 'workspace:export', label: 'Workspace export', description: 'Lets another Orbit instance import this workspace as you see it: tasks, docs, chat and files.' },
  { value: 'workspace:export:personal', label: 'Personal export', description: 'Lets another Orbit instance import only your private docs, your private channels and your DMs.' },
] as const

type TokenKind = typeof TOKEN_KINDS[number]['value']

function tokenResources(serviceAccount: boolean, projects: ReactNode): ScopeResource<ApiTokenScope>[] {
  return [
    { name: 'Tasks', hint: 'In the selected projects. Write includes delete.', read: 'tasks:read', write: 'tasks:write', details: projects },
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
  const canExportPersonal = useCan('workspace.export_personal')
  const tokens = useApiTokens(workspace.id, canManage || canExportPersonal)
  const projectsQuery = useProjects(workspace.id, canManage)
  const projects = projectsQuery.data ?? []
  const createToken = useCreateApiToken(workspace.id)
  const revokeToken = useRevokeApiToken(workspace.id)
  const [name, setName] = useState('')
  const [projectIds, setProjectIds] = useState<string[]>([])
  const [allProjects, setAllProjects] = useState(false)
  const projectNames = new Map(projects.map((project) => [project.id, project.name]))
  const [scopes, setScopes] = useState<ApiTokenScope[]>(['tasks:read'])
  const [chosenKind, setKind] = useState<TokenKind>('access')
  const kind: TokenKind = canManage ? chosenKind : 'workspace:export:personal'
  const [serviceAccount, setServiceAccount] = useState(false)
  const [expiration, setExpiration] = useState<ExpirationValue>('never')
  const [issuedToken, setIssuedToken] = useState<string>()
  const [copied, setCopied] = useState(false)

  const access = kind === 'access'
  const kindDescription = TOKEN_KINDS.find((option) => option.value === kind)?.description
  // Projects only limit task access, so a token without a task scope needs none.
  const taskScope = access && scopes.some((scope) => scope.startsWith('tasks:'))
  const ready = name.trim() !== '' && (!access || (scopes.length > 0 && (!taskScope || allProjects || projectIds.length > 0)))

  if (!canManage && !canExportPersonal) return <SettingsCard title="API tokens"><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage API tokens.</p></SettingsCard>

  // A service account has task access only, so selecting it drops the docs and chat scopes.
  function selectServiceAccount(checked: boolean) {
    setServiceAccount(checked)
    if (!checked) return
    const taskScopes = scopes.filter((scope) => scope.startsWith('tasks:'))
    setScopes(taskScopes.length ? taskScopes : ['tasks:read'])
  }

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (!ready) return
    const issued = await createToken.mutateAsync({ name: name.trim(), projectIds: taskScope && !allProjects ? projectIds : [], allProjects: taskScope && allProjects, scopes: access ? scopes : [kind], serviceAccount: access && serviceAccount, expiresInDays: expiration === 'never' ? null : Number(expiration) })
    setIssuedToken(issued.token)
    setName('')
    setCopied(false)
  }

  return <>
    <SettingsCard title="Create API token" description={canManage ? 'Create a personal token, or select a non-login service account for an integration. The secret is shown only once.' : `${kindDescription} Paste it with this workspace's URL in the other instance. The secret is shown only once.`}>
      <form onSubmit={(event) => void submit(event)}>
        <FieldGrid>
        <Field><FieldLabel htmlFor="api-token-name">Name <RequiredMark /></FieldLabel><Input id="api-token-name" value={name} maxLength={100} onChange={(event) => setName(event.target.value)} placeholder={access ? 'Discord bot' : 'Orbit import'} /></Field>
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
        {canManage ? <Field className="min-[900px]:col-span-2">
          <FieldLabel htmlFor="api-token-kind" className="items-center gap-1.5">Token type {kindDescription ? <InfoTip text={kindDescription} /> : null}</FieldLabel>
          <Select items={TOKEN_KINDS} value={kind} onValueChange={(value) => setKind(value as TokenKind)}>
            <SelectTrigger id="api-token-kind" aria-label="Token type">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TOKEN_KINDS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </Field> : null}
        {access ? <>
        {/* a choice card: the whole card toggles the checkbox and tints while checked */}
        <FieldLabel className="min-[900px]:col-span-2">
          <Field orientation="horizontal">
            <Checkbox checked={serviceAccount} onCheckedChange={selectServiceAccount} />
            <FieldContent>
              <FieldTitle className="gap-1.5">Use a service account <InfoTip text="For bots, such as a Discord bot. Changes show as made by this account, not by you. Tokens with the same name share one account." /></FieldTitle>
            </FieldContent>
          </Field>
        </FieldLabel>
        <div className="min-[900px]:col-span-2">
          <AccessPicker id="api-token-access" resources={tokenResources(serviceAccount, taskScope ? <>
            <ProjectPicker id="api-token-projects" projects={projects} selected={projectIds} onChange={setProjectIds} all={allProjects} onAllChange={setAllProjects} empty={projectsQuery.isPending ? 'Loading projects…' : projects.length ? undefined : 'This workspace has no active projects.'} />
            {scopes.includes('tasks:write') ? <Label className="mt-3 flex items-start gap-2">
              <Checkbox className="mt-0.5" checked={scopes.includes('tasks:import')} onCheckedChange={(checked) => setScopes(checked ? [...scopes, 'tasks:import'] : scopes.filter((scope) => scope !== 'tasks:import'))} />
              <span className="grid gap-0.5">Import mode<span className="text-xs font-normal text-muted-foreground">Keeps original dates, numbers and authors. Sends no notifications.</span></span>
            </Label> : null}
          </> : null)} scopes={scopes} onChange={(next) => setScopes(next.includes('tasks:write') ? next : next.filter((scope) => scope !== 'tasks:import'))} />
        </div>
        </> : null}
        <Button className="justify-self-start" type="submit" disabled={createToken.isPending || !ready}>Create token</Button>
        {createToken.isError ? <p className="text-destructive" role="alert">The API token could not be created.</p> : null}
        </FieldGrid>
      </form>
      {issuedToken ? <div className="mt-[18px] grid gap-2.5 rounded-lg border border-primary bg-primary/10 p-3 text-xs" role="status"><strong>Copy this token now. You cannot see it again.</strong><div className="flex items-center gap-2"><code className="min-w-0 flex-1 [overflow-wrap:anywhere] select-all">{issuedToken}</code><Button variant="outline" type="button" onClick={() => { void navigator.clipboard.writeText(issuedToken).then(() => { setCopied(true); toast('Token copied') }, () => toast.error('Could not copy the token.')) }}><Copy className="size-4" /> {copied ? 'Copied' : 'Copy'}</Button></div></div> : null}
    </SettingsCard>
    <SettingsCard title="Active API tokens" flush>
      <div className="flex flex-col divide-y">
        {tokens.isPending ? <SettingsRow><LoadingText>Loading API tokens…</LoadingText></SettingsRow> : null}
        {tokens.isError ? <SettingsRow role="alert">API tokens could not be loaded.</SettingsRow> : null}
        {tokens.data?.length === 0 ? <SettingsRow>No active API tokens.</SettingsRow> : null}
        {tokens.data?.map((token) => <SettingsRow className="flex-nowrap" key={token.id}>
          <RowIcon><Key /></RowIcon>
          <div className="flex min-w-0 flex-1 flex-col gap-2 sm:flex-row sm:items-center sm:gap-3">
            <div className="flex min-w-0 flex-1 flex-col gap-0.5"><span className="flex items-center gap-2 text-[13px] font-medium text-foreground">{token.name}{isExpired(token.expires_at) ? <Badge variant="destructive">Expired</Badge> : null}</span><span className="text-xs text-muted-foreground/70">{token.service_account_name ? `Service account: ${token.service_account_name}` : 'Personal token'} · <code>{token.token_prefix}…</code> · {token.all_projects ? 'All projects · ' : token.project_ids.length ? `${token.project_ids.map((id) => projectNames.get(id) ?? 'Unavailable project').join(', ')} · ` : null}{token.expires_at ? `${isExpired(token.expires_at) ? 'expired' : 'expires'} ${shortDate(token.expires_at)}` : 'never expires'} · created {relativeTime(token.created_at)}</span></div>
            {/* Phones have no room beside the text, so the badges go under it, as in the mobile app. */}
            <span className="flex flex-wrap gap-1">{token.scopes.map((scope) => <Badge variant="secondary" key={scope}>{scope}</Badge>)}</span>
          </div>
          <Button variant="ghost" type="button" disabled={revokeToken.isPending} onClick={async () => { if (await confirmAction({ title: `Revoke ${token.name}?`, description: 'The integration will lose access immediately.', confirmLabel: 'Revoke', danger: true })) revokeToken.mutate(token.id) }}>Revoke</Button>
        </SettingsRow>)}
      </div>
    </SettingsCard>
  </>
}
