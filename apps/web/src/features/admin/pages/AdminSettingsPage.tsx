import { useState } from 'react'
import { toast } from 'sonner'
import type { LimitsView, SmtpSecurity, SmtpView } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useCurrentUser } from '@/features/auth/api'
import { useInstanceSettings, useRemoveSmtp, useSaveLimits, useSaveSmtp, useSendTestEmail, useSetRegistration } from '@/features/admin/api'
import { FieldGrid, RequiredMark, SettingsRow } from '@/features/settings/components/SettingsParts'

// `items` lets Select.Value render the option label instead of the raw value.
const SECURITY_OPTIONS: { value: SmtpSecurity; label: string }[] = [
  { value: 'starttls', label: 'STARTTLS (port 587)' },
  { value: 'tls', label: 'TLS (port 465)' },
  { value: 'none', label: 'None (local relay only)' },
]
const DEFAULT_PORT: Record<SmtpSecurity, number> = { starttls: 587, tls: 465, none: 25 }

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** Instance settings: who can create an account, the mail server for invitations, sign-up and password reset, and
 * the limits per account and workspace. */
export function AdminSettingsPage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="General"><p role="status">Loading settings…</p></SettingsCard>
  if (settings.isError) return <SettingsCard title="General"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  const smtp = settings.data.smtp ?? null
  return (
    <>
      <RegistrationCard open={settings.data.registration_open} emailReady={smtp !== null} />
      {/* Remount on save so the form shows what the server stored. */}
      <EmailCard key={JSON.stringify(smtp)} smtp={smtp} />
      <LimitsCard key={JSON.stringify(settings.data.limits)} limits={settings.data.limits} />
    </>
  )
}

function RegistrationCard({ open, emailReady }: { open: boolean; emailReady: boolean }) {
  const setRegistration = useSetRegistration()
  return (
    <SettingsCard title="Registration" description="Who can create an account on this Orbit." flush>
      <SettingsRow className="flex-nowrap">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
          <span className="font-medium text-foreground">Open registration</span>
          <span className="text-xs text-muted-foreground">
            {emailReady
              ? 'On: anyone can create an account with a link sent to their email. Off: people join only by invitation.'
              : 'Save a mail server below first: the sign-up link goes out by email.'}
          </span>
        </div>
        <Switch
          aria-label="Open registration"
          checked={open}
          disabled={(!emailReady && !open) || setRegistration.isPending}
          onCheckedChange={(checked: boolean) => setRegistration.mutate(checked, {
            onSuccess: () => toast(checked ? 'Registration is open' : 'Registration is closed'),
            onError: (error) => toast.error(problemText(error, 'Registration could not be changed.')),
          })}
        />
      </SettingsRow>
    </SettingsCard>
  )
}

function EmailCard({ smtp }: { smtp: SmtpView | null }) {
  const currentUser = useCurrentUser()
  const save = useSaveSmtp()
  const remove = useRemoveSmtp()
  const test = useSendTestEmail()
  const [draft, setDraft] = useState({
    host: smtp?.host ?? '',
    port: String(smtp?.port ?? DEFAULT_PORT.starttls),
    security: smtp?.security ?? 'starttls' as SmtpSecurity,
    username: smtp?.username ?? '',
    password: '',
    from_address: smtp?.from_address ?? '',
    from_name: smtp?.from_name ?? '',
  })
  const set = (field: keyof typeof draft) => (event: React.ChangeEvent<HTMLInputElement>) => setDraft((current) => ({ ...current, [field]: event.target.value }))

  function submit(event: React.FormEvent) {
    event.preventDefault()
    save.mutate({
      host: draft.host.trim(),
      port: Number(draft.port),
      security: draft.security,
      username: draft.username.trim() || null,
      // An empty field keeps the saved password.
      password: draft.password || undefined,
      from_address: draft.from_address.trim(),
      from_name: draft.from_name.trim() || null,
    }, {
      onSuccess: () => toast('Mail settings saved'),
      onError: (error) => toast.error(problemText(error, 'Mail settings could not be saved.')),
    })
  }

  async function removeServer() {
    const confirmed = await confirmAction({
      title: 'Remove the mail server?',
      description: 'Orbit stops sending invitations and password reset links by email, and registration closes.',
      confirmLabel: 'Remove',
      danger: true,
    })
    if (!confirmed) return
    remove.mutate(undefined, {
      onSuccess: () => toast('Mail server removed'),
      onError: (error) => toast.error(problemText(error, 'The mail server could not be removed.')),
    })
  }

  return (
    <form onSubmit={submit}>
      <SettingsCard
        title="Email"
        description="The SMTP server Orbit sends invitations, sign-up links and password reset links through. Nothing else is sent."
        actions={smtp ? (
          <>
            <Button type="button" variant="outline" disabled={test.isPending} onClick={() => test.mutate(undefined, {
              onSuccess: () => toast(`Test email sent to ${currentUser.data?.email ?? 'you'}`),
              onError: (error) => toast.error(problemText(error, 'The test email could not be sent.')),
            })}>
              {test.isPending ? 'Sending…' : 'Send test email'}
            </Button>
            <Button type="button" variant="ghost" className="text-destructive" disabled={remove.isPending} onClick={() => void removeServer()}>Remove</Button>
          </>
        ) : undefined}
      >
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="smtp-host">Host <RequiredMark /></FieldLabel>
            <Input id="smtp-host" required placeholder="smtp.example.com" value={draft.host} onChange={set('host')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="smtp-security">Encryption</FieldLabel>
            <Select items={SECURITY_OPTIONS} value={draft.security} onValueChange={(value) => {
              const security = value as SmtpSecurity
              setDraft((current) => ({ ...current, security, port: String(DEFAULT_PORT[security]) }))
            }}>
              <SelectTrigger id="smtp-security"><SelectValue /></SelectTrigger>
              <SelectContent>
                {SECURITY_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <FieldLabel htmlFor="smtp-port">Port <RequiredMark /></FieldLabel>
            <Input id="smtp-port" required type="number" min={1} max={65535} value={draft.port} onChange={set('port')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="smtp-username">Username</FieldLabel>
            <Input id="smtp-username" autoComplete="off" value={draft.username} onChange={set('username')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="smtp-password">Password</FieldLabel>
            <PasswordInput id="smtp-password" autoComplete="new-password" placeholder={smtp?.password_set ? 'Saved. Leave empty to keep it.' : ''} value={draft.password} onChange={set('password')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="smtp-from-address">From address <RequiredMark /></FieldLabel>
            <Input id="smtp-from-address" required type="email" placeholder="orbit@example.com" value={draft.from_address} onChange={set('from_address')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="smtp-from-name">From name</FieldLabel>
            <Input id="smtp-from-name" placeholder="Orbit" value={draft.from_name} onChange={set('from_name')} />
          </Field>
        </FieldGrid>
        <div className="mt-4 flex justify-end">
          <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Saving…' : 'Save mail settings'}</Button>
        </div>
      </SettingsCard>
    </form>
  )
}

const BYTES_PER_GB = 1_000_000_000

/** A whole number above 0, or `null` (unlimited) for an empty field or 0. `undefined` when it is not a number. */
function parseLimit(value: string, scale = 1): number | null | undefined {
  if (!value.trim()) return null
  const number = Number(value)
  if (!Number.isFinite(number) || number < 0) return undefined
  return number === 0 ? null : Math.round(number * scale)
}

function LimitsCard({ limits }: { limits: LimitsView }) {
  const save = useSaveLimits()
  const [workspaces, setWorkspaces] = useState(limits.max_owned_workspaces ? String(limits.max_owned_workspaces) : '')
  const [storage, setStorage] = useState(limits.workspace_storage_quota_bytes ? String(limits.workspace_storage_quota_bytes / BYTES_PER_GB) : '')

  function submit(event: React.FormEvent) {
    event.preventDefault()
    const maxOwnedWorkspaces = parseLimit(workspaces)
    const storageQuota = parseLimit(storage, BYTES_PER_GB)
    if (maxOwnedWorkspaces === undefined || storageQuota === undefined) {
      toast.error('Enter a number, or leave the field empty for no limit.')
      return
    }
    save.mutate({ max_owned_workspaces: maxOwnedWorkspaces, workspace_storage_quota_bytes: storageQuota }, {
      onSuccess: () => toast('Limits saved'),
      onError: (error) => toast.error(problemText(error, 'The limits could not be saved.')),
    })
  }

  return (
    <form onSubmit={submit}>
      <SettingsCard title="Limits" description="What one account and one workspace may use. Empty or 0 means no limit. A lower limit removes nothing that exists.">
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="limit-workspaces">Workspaces per account</FieldLabel>
            <Input id="limit-workspaces" type="number" min={0} step={1} placeholder="No limit" value={workspaces} onChange={(event) => setWorkspaces(event.target.value)} />
            <FieldDescription>Workspaces one account may own, not counting the trash. Admins have no limit.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="limit-storage">Storage per workspace (GB)</FieldLabel>
            <Input id="limit-storage" type="number" min={0} step="any" placeholder="No limit" value={storage} onChange={(event) => setStorage(event.target.value)} />
            <FieldDescription>Files, emoji and stickers of one workspace, each file counted once.</FieldDescription>
          </Field>
        </FieldGrid>
        <div className="mt-4 flex justify-end">
          <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Saving…' : 'Save limits'}</Button>
        </div>
      </SettingsCard>
    </form>
  )
}
