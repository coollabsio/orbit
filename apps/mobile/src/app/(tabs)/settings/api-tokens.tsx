import type { ApiTokenRecord, ApiTokenScope } from '@orbit/api/generated/types.gen'
import * as Clipboard from 'expo-clipboard'
import { type ReactNode, useState } from 'react'
import { Pressable, View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { Badge, FieldLabel, RowIcon, Select, SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { Toggles } from '../../../components/settings/Toggles'
import { relativeTime, shortDate } from '../../../data/format'
import { type AccessLevel, accessLevel, EXPIRATION_OPTIONS, type ExpirationValue, isExpired, levelEnabled, type ScopeResource, TOKEN_KINDS, type TokenKind, tokenResources, tokenSummary, withAccessLevel } from '../../../data/workspace-settings-api'
import { useApiTokens, useApiTokenWrites } from '../../../hooks/workspace-settings'
import { useOrbit } from '../../../orbit-context'
import { useProjects } from '../../../queries'
import { useTheme } from '../../../theme'
import { Button, Icon, Input, Text, LabelTip } from '../../../ui'

const LEVELS: { value: AccessLevel; label: string }[] = [
  { value: 'none', label: 'No access' },
  { value: 'read', label: 'Read' },
  { value: 'write', label: 'Write' },
]

/** Personal and service account tokens for integrations: create one (its secret shows once), list and revoke them. */
export default function ApiTokensScreen() {
  const { workspace } = useOrbit()
  const canManage = workspace.permissions.includes('api_tokens.manage')
  const canCreate = canManage || workspace.permissions.includes('workspace.export_personal')
  const tokens = useApiTokens(canCreate)
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={canCreate ? () => tokens.refetch() : undefined}>
      {canCreate ? (
        <>
          <CreateToken canManage={canManage} />
          <ActiveTokens />
        </>
      ) : (
        <SettingsCard title="API tokens">
          <Text className="text-[14px] leading-[20px] text-muted-foreground">Only workspace owners and administrators can manage API tokens.</Text>
        </SettingsCard>
      )}
    </SettingsShell>
  )
}

function CreateToken({ canManage }: { canManage: boolean }) {
  const { create } = useApiTokenWrites()
  const projectsQuery = useProjects()
  const projects = projectsQuery.data ?? []
  const [name, setName] = useState('')
  const [projectIds, setProjectIds] = useState<string[]>([])
  const [allProjects, setAllProjects] = useState(false)
  const [scopes, setScopes] = useState<ApiTokenScope[]>(['tasks:read'])
  const [chosenKind, setKind] = useState<TokenKind>('access')
  // members may create only a personal export token
  const kind: TokenKind = canManage ? chosenKind : 'workspace:export:personal'
  const access = kind === 'access'
  const kindDescription = TOKEN_KINDS.find((option) => option.value === kind)?.description
  const [serviceAccount, setServiceAccount] = useState(false)
  const [expiration, setExpiration] = useState<ExpirationValue>('never')
  const [issuedToken, setIssuedToken] = useState<string>()
  const [copied, setCopied] = useState(false)

  // projects only limit task access, so a token without a task scope needs none
  const taskScope = access && scopes.some((scope) => scope.startsWith('tasks:'))
  const ready = name.trim() !== '' && (!access || (scopes.length > 0 && (!taskScope || allProjects || projectIds.length > 0)))

  // a service account has task access only, so selecting it drops the docs and chat scopes
  const selectServiceAccount = (checked: boolean) => {
    setServiceAccount(checked)
    if (!checked) return
    const taskScopes = scopes.filter((scope) => scope.startsWith('tasks:'))
    setScopes(taskScopes.length ? taskScopes : ['tasks:read'])
  }

  const submit = () => {
    if (!ready || create.isPending) return
    create.mutate(
      { name: name.trim(), projectIds: taskScope && !allProjects ? projectIds : [], allProjects: taskScope && allProjects, scopes: access ? scopes : [kind], serviceAccount: access && serviceAccount, expiresInDays: expiration === 'never' ? null : Number(expiration) },
      {
        onSuccess: (issued) => {
          setIssuedToken(issued.token)
          setName('')
          setCopied(false)
        },
      },
    )
  }

  const projectPicker = taskScope ? (
    <ProjectPicker
      projects={projects}
      selected={projectIds}
      onChange={setProjectIds}
      all={allProjects}
      onAllChange={setAllProjects}
      empty={projectsQuery.isPending ? 'Loading projects…' : projects.length ? undefined : 'This workspace has no active projects.'}
    />
  ) : null

  return (
    <SettingsCard title="Create API token" description={canManage ? 'Create a personal token, or select a non-login service account for an integration. The secret is shown only once.' : `${kindDescription} Paste it with this workspace's URL in the other instance. The secret is shown only once.`}>
      <View className="gap-4">
        <Input label="Name" required value={name} maxLength={100} onChangeText={setName} placeholder={access ? 'Discord bot' : 'Orbit import'} autoCorrect={false} returnKeyType="done" onSubmitEditing={submit} />
        <View className="gap-2">
          <FieldLabel>Expiration</FieldLabel>
          <Select label="Expiration" value={expiration} options={EXPIRATION_OPTIONS} onChange={setExpiration} />
        </View>
        {canManage ? (
          <View className="gap-2">
            {kindDescription ? <LabelTip label="Token type" text={kindDescription} /> : <FieldLabel>Token type</FieldLabel>}
            <Select label="Token type" value={kind} options={TOKEN_KINDS} onChange={setKind} />
          </View>
        ) : null}
        {access ? (
          <>
            {/* a choice card: the whole card toggles the checkbox and tints while checked */}
            <Pressable
              accessibilityRole="checkbox"
              accessibilityLabel="Use a service account"
              accessibilityState={{ checked: serviceAccount }}
              onPress={() => selectServiceAccount(!serviceAccount)}
              className={`flex-row gap-2 rounded-lg border p-2.5 ${serviceAccount ? 'border-primary/20 bg-primary/10' : 'border-border active:bg-muted/50'}`}
            >
              <View className="pt-0.5">
                <Checkbox checked={serviceAccount} />
              </View>
              <View className="flex-1">
                <LabelTip label="Use a service account" text="For bots, such as a Discord bot. Changes show as made by this account, not by you. Tokens with the same name share one account." />
              </View>
            </Pressable>
            <AccessPicker resources={tokenResources(serviceAccount)} scopes={scopes} onChange={setScopes} details={{ Tasks: projectPicker }} />
          </>
        ) : null}
        <View className="items-start">
          <Button label="Create token" disabled={create.isPending || !ready} onPress={submit} />
        </View>
        {create.isError ? <Text className="text-[14px] leading-[20px] text-destructive">The API token could not be created.</Text> : null}
      </View>
      {issuedToken ? (
        <View accessibilityLiveRegion="polite" className="mt-[18px] gap-2.5 rounded-lg border border-primary bg-primary/10 p-3">
          <Text className="font-noto-semibold text-[12px] leading-[16px]">Copy this token now. You cannot see it again.</Text>
          <View className="flex-row items-center gap-2">
            <Text selectable className="min-w-0 flex-1 font-mono text-[12px] leading-[16px]">
              {issuedToken}
            </Text>
            <Button
              label={copied ? 'Copied' : 'Copy'}
              icon="copy"
              variant="outline"
              onPress={() => void Clipboard.setStringAsync(issuedToken).then((done) => setCopied(done), () => setCopied(false))}
            />
          </View>
        </View>
      ) : null}
    </SettingsCard>
  )
}

function ActiveTokens() {
  const tokens = useApiTokens(true)
  const projects = useProjects().data ?? []
  const projectNames = new Map(projects.map((project) => [project.id, project.name]))
  const { revoke } = useApiTokenWrites()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const status = tokens.isPending ? 'Loading API tokens…' : tokens.isError ? 'API tokens could not be loaded.' : tokens.data.length === 0 ? 'No active API tokens.' : null
  return (
    <SettingsCard title="Active API tokens" flush>
      {status ? (
        <SettingsRow first>
          <Text className={`text-[14px] leading-[20px] ${tokens.isError ? 'text-destructive' : 'text-foreground'}`}>{status}</Text>
        </SettingsRow>
      ) : null}
      {tokens.data?.map((token, index) => (
        <TokenRow
          key={token.id}
          token={token}
          first={index === 0}
          projectNames={projectNames}
          disabled={revoke.isPending}
          onRevoke={() => setConfirm({ title: `Revoke ${token.name}?`, description: 'The integration will lose access immediately.', confirmLabel: 'Revoke', danger: true, onConfirm: () => revoke.mutate(token.id) })}
        />
      ))}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}

function TokenRow({ token, first, projectNames, disabled, onRevoke }: { token: ApiTokenRecord; first: boolean; projectNames: ReadonlyMap<string, string>; disabled: boolean; onRevoke: () => void }) {
  const expired = isExpired(token.expires_at)
  const expiry = token.expires_at ? `${expired ? 'expired' : 'expires'} ${shortDate(token.expires_at)}` : 'never expires'
  return (
    <SettingsRow first={first}>
      <RowIcon icon="key" />
      <View className="min-w-0 flex-1 gap-0.5">
        <View className="flex-row flex-wrap items-center gap-2">
          <Text className="font-noto-medium text-[13px] text-foreground">{token.name}</Text>
          {expired ? <Badge label="Expired" tone="destructive" /> : null}
        </View>
        <Text className="text-[12px] text-muted-foreground/70">{tokenSummary(token, projectNames, relativeTime(token.created_at), expiry)}</Text>
        {/* the web's scope badges sit beside the text; a phone row has no room, so they wrap under it */}
        <View className="mt-1 flex-row flex-wrap gap-1">
          {token.scopes.map((scope) => (
            <Badge key={scope} label={scope} tone="secondary" />
          ))}
        </View>
      </View>
      <Button label="Revoke" variant="ghost" disabled={disabled} onPress={onRevoke} />
    </SettingsRow>
  )
}

/** The web `AccessPicker`: one row per resource with No access / Read / Write; `details` shows under a row, such as the project list of Tasks. */
function AccessPicker({ resources, scopes, onChange, details }: { resources: ScopeResource[]; scopes: ApiTokenScope[]; onChange: (scopes: ApiTokenScope[]) => void; details: Record<string, ReactNode> }) {
  return (
    <View accessibilityLabel="Access" className="gap-3">
      {resources.map((resource) => (
        <View key={resource.name} className="gap-3">
          <View className="flex-row items-center justify-between gap-3">
            <View className="min-w-0 flex-1">
              <LabelTip label={resource.name} text={resource.hint} className="font-noto-medium text-[14px] leading-[20px]" />
            </View>
            <Toggles
              filled
              label={resource.name}
              value={accessLevel(resource, scopes)}
              options={LEVELS.map((level) => ({ ...level, disabled: !levelEnabled(resource, level.value) }))}
              onChange={(level) => onChange(withAccessLevel(resource, scopes, level))}
            />
          </View>
          {details[resource.name] ? <View className="border-l border-border pl-4">{details[resource.name]}</View> : null}
        </View>
      ))}
    </View>
  )
}

/** The project checklist under the Tasks row, with "Select all" above two or more projects. `all` covers every project, including future ones, and hides the list. */
function ProjectPicker({ projects, selected, onChange, all, onAllChange, empty }: { projects: { id: string; name: string }[]; selected: string[]; onChange: (ids: string[]) => void; all: boolean; onAllChange: (all: boolean) => void; empty?: string }) {
  const some = selected.length > 0 && selected.length < projects.length
  return (
    <View accessibilityLabel="Projects" className="gap-3">
      <CheckRow label="All projects, including future ones" checked={all} onPress={() => onAllChange(!all)} />
      {all ? null : (
        <>
          {projects.length > 1 ? (
            <CheckRow label="Select all" muted checked={selected.length === projects.length} indeterminate={some} onPress={() => onChange(selected.length === projects.length ? [] : projects.map((project) => project.id))} />
          ) : null}
          {projects.map((project) => {
            const checked = selected.includes(project.id)
            return <CheckRow key={project.id} label={project.name} checked={checked} onPress={() => onChange(checked ? selected.filter((other) => other !== project.id) : [...selected, project.id])} />
          })}
          {empty ? <Text className="text-[14px] leading-[20px] text-muted-foreground">{empty}</Text> : null}
        </>
      )}
    </View>
  )
}

function CheckRow({ label, checked, indeterminate = false, muted = false, onPress }: { label: string; checked: boolean; indeterminate?: boolean; muted?: boolean; onPress: () => void }) {
  return (
    <Pressable accessibilityRole="checkbox" accessibilityLabel={label} accessibilityState={{ checked: indeterminate ? 'mixed' : checked }} onPress={onPress} hitSlop={4} className="flex-row items-center gap-2">
      <Checkbox checked={checked} indeterminate={indeterminate} />
      <Text className={`text-[14px] leading-[18px] ${muted ? 'text-muted-foreground' : 'font-noto-medium text-foreground'}`}>{label}</Text>
    </Pressable>
  )
}

/** The web `Checkbox`: a 16px box, primary with a check (or a dash) when on. */
function Checkbox({ checked, indeterminate = false }: { checked: boolean; indeterminate?: boolean }) {
  const { colors } = useTheme()
  const on = checked || indeterminate
  return (
    <View className={`size-4 items-center justify-center rounded-[4px] border ${on ? 'border-primary bg-primary' : 'border-input dark:bg-input/30'}`}>
      {on ? <Icon name={indeterminate ? 'minus' : 'check'} size={14} color={colors.primaryForeground} /> : null}
    </View>
  )
}
