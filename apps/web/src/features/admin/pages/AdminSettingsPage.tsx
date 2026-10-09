import { useState } from 'react'
import { toast } from 'sonner'
import type { SmtpSecurity, SmtpView } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useCurrentUser } from '@/features/auth/api'
import { useInstanceSettings, useRemoveSmtp, useSaveSmtp, useSendTestEmail, useSetRegistration } from '@/features/admin/api'
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

/** Instance settings: who can create an account, and the mail server for invitations, sign-up and password reset. */
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
