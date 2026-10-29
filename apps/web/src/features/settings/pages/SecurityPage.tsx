import { useId, useState } from 'react'
import { flushSync } from 'react-dom'
import { toast } from 'sonner'
import { Copy, DocumentDownload, FingerScan, Mobile } from 'reicon-react'
import { ApiProblem } from '@orbit/api/problem'
import type { ConnectedAccount, PasskeyRecord, Provider, TotpSetup } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { ProviderMark } from '@/components/common/icons/ProviderMark'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { useAuthOptions, useConnectedAccounts, useCurrentUser, useDisconnectAccount, useStartProviderSignIn } from '@/features/auth/api'
import { ConfirmWithProvider } from '@/features/auth/components/ConfirmWithProvider'
import { defaultPasskeyName, isPasskeyCancel, passkeysSupported } from '@/features/auth/passkey'
import { PROVIDER_NAMES } from '@/features/auth/providerSignIn'
import {
  useAddPasskey,
  useDeletePasskey,
  useDisableTwoFactor,
  useEnableTotp,
  usePasskeys,
  useRegenerateRecoveryCodes,
  useSetupTotp,
  useTwoFactor,
} from '@/features/settings/api/security'
import { RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { relativeTime } from '@/lib/format'

function problemDetail(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** "5m ago", "3d ago", or "on Sep 4" for older dates. */
function ago(iso: string) {
  const relative = relativeTime(iso)
  if (relative === 'now') return 'just now'
  return /^\d+[mhd]$/.test(relative) ? `${relative} ago` : `on ${relative}`
}

/** Where `connect` and `confirm` flows come back to. */
const SECURITY_PATH = '/profile/security'

/** Accounts created with Google or GitHub have no password until they set one. */
function useHasPassword() {
  return useCurrentUser().data?.has_password !== false
}

/** Account view: the authenticator app, recovery codes, passkeys and connected Google / GitHub accounts. */
export function SecurityPage() {
  return (
    <>
      <AuthenticatorCard />
      <PasskeysCard />
      <ConnectedAccountsCard />
    </>
  )
}

type AuthenticatorDialog = 'setup' | 'regenerate' | 'disable' | null

function AuthenticatorCard() {
  const status = useTwoFactor()
  const [dialog, setDialog] = useState<AuthenticatorDialog>(null)
  const enabled = status.data?.totp_enabled === true
  const close = () => setDialog(null)

  return (
    <SettingsCard
      title="Authenticator app"
      description="Sign-in asks for a code from an app such as 1Password, Authy or Google Authenticator after your password."
      actions={status.data ? (
        enabled ? (
          <>
            <Button type="button" variant="outline" onClick={() => setDialog('regenerate')}>New recovery codes</Button>
            <Button type="button" variant="destructive" onClick={() => setDialog('disable')}>Turn off</Button>
          </>
        ) : (
          <Button type="button" variant="outline" onClick={() => setDialog('setup')}>Set up</Button>
        )
      ) : undefined}
      flush
    >
      {status.isPending ? <SettingsRow role="status">Loading…</SettingsRow> : null}
      {status.isError ? <SettingsRow role="alert">Two-factor sign-in could not be loaded. <Button variant="ghost" onClick={() => void status.refetch()}>Retry</Button></SettingsRow> : null}
      {status.data ? (
        <SettingsRow className="flex-nowrap">
          <RowIcon><Mobile /></RowIcon>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="flex items-center gap-2 text-[13px] font-medium text-foreground">
              Two-factor sign-in
              {enabled ? <Badge>On</Badge> : <Badge variant="secondary">Off</Badge>}
            </span>
            <span className="text-xs text-muted-foreground/70">
              {enabled
                ? `On · ${status.data.recovery_codes_left} recovery ${status.data.recovery_codes_left === 1 ? 'code' : 'codes'} left`
                : 'Only your password is needed to sign in.'}
            </span>
          </div>
        </SettingsRow>
      ) : null}
      {dialog === 'setup' ? <SetupTotpDialog onClose={close} /> : null}
      {dialog === 'regenerate' ? <RegenerateCodesDialog onClose={close} /> : null}
      {dialog === 'disable' ? <DisableTwoFactorDialog onClose={close} /> : null}
    </SettingsCard>
  )
}

/**
 * The current password, asked again before a change to how the account signs in. An account without one has no
 * field: when its sign-in is too old, the server asks for a fresh provider sign-in instead.
 */
function CurrentPassword({ hasPassword, value, onChange, error, fallback }: { hasPassword: boolean; value: string; onChange: (value: string) => void; error: unknown; fallback: string }) {
  const id = useId()
  const message = error ? problemDetail(error, fallback) : null
  if (!hasPassword) {
    if (error instanceof ApiProblem && error.code === 'reauthentication_required') return <ConfirmWithProvider returnTo={SECURITY_PATH} />
    return message ? <p role="alert" className="text-[13px] text-destructive">{message}</p> : null
  }
  return (
    <Field data-invalid={message ? true : undefined}>
      <FieldLabel htmlFor={id}>Current password</FieldLabel>
      <PasswordInput id={id} required autoFocus autoComplete="current-password" aria-invalid={message ? true : undefined} value={value} onChange={(event) => onChange(event.target.value)} />
      {message ? <FieldError>{message}</FieldError> : null}
    </Field>
  )
}

function SetupTotpDialog({ onClose }: { onClose: () => void }) {
  const setup = useSetupTotp()
  const enable = useEnableTotp()
  const hasPassword = useHasPassword()
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [secret, setSecret] = useState<TotpSetup | null>(null)
  const [codes, setCodes] = useState<string[] | null>(null)
  const codeId = useId()

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent>
        {codes ? (
          <RecoveryCodesStep codes={codes} title="Two-factor sign-in is on" onDone={onClose} />
        ) : secret ? (
          <form className="grid gap-4" onSubmit={(event) => {
            event.preventDefault()
            enable.mutate(code.trim(), { onSuccess: setCodes })
          }}>
            <DialogHeader>
              <DialogTitle>Scan the QR code</DialogTitle>
              <DialogDescription>Scan it with your authenticator app, or enter the key by hand. Then enter the code the app shows.</DialogDescription>
            </DialogHeader>
            <img src={secret.qr_code} alt="QR code for your authenticator app" className="size-48 justify-self-center rounded-lg bg-white p-3" />
            <CopyField label="Setup key" value={secret.secret} />
            <Field data-invalid={enable.isError ? true : undefined}>
              <FieldLabel htmlFor={codeId}>Code from the app</FieldLabel>
              <Input id={codeId} required autoComplete="one-time-code" inputMode="numeric" spellCheck={false} aria-invalid={enable.isError ? true : undefined} value={code} onChange={(event) => setCode(event.target.value)} />
              {enable.isError ? <FieldError>{problemDetail(enable.error, 'Two-factor sign-in could not be turned on.')}</FieldError> : null}
            </Field>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
              <Button type="submit" disabled={enable.isPending || !code.trim()}>{enable.isPending ? 'Turning on…' : 'Turn on'}</Button>
            </DialogFooter>
          </form>
        ) : (
          <form className="grid gap-4" onSubmit={(event) => {
            event.preventDefault()
            setup.mutate(hasPassword ? password : null, { onSuccess: setSecret })
          }}>
            <DialogHeader>
              <DialogTitle>Set up an authenticator app</DialogTitle>
              <DialogDescription>{hasPassword ? 'Confirm your password to start.' : 'Orbit makes a key for your authenticator app.'}</DialogDescription>
            </DialogHeader>
            <CurrentPassword hasPassword={hasPassword} value={password} onChange={setPassword} error={setup.error} fallback="The setup could not start." />
            <DialogFooter>
              <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
              <Button type="submit" disabled={setup.isPending || (hasPassword && !password)}>{setup.isPending ? 'Please wait…' : 'Continue'}</Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  )
}

function RegenerateCodesDialog({ onClose }: { onClose: () => void }) {
  const regenerate = useRegenerateRecoveryCodes()
  const hasPassword = useHasPassword()
  const [password, setPassword] = useState('')
  const [codes, setCodes] = useState<string[] | null>(null)
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent>
        {codes ? (
          <RecoveryCodesStep codes={codes} title="New recovery codes" onDone={onClose} />
        ) : (
          <form className="grid gap-4" onSubmit={(event) => {
            event.preventDefault()
            regenerate.mutate(hasPassword ? password : null, { onSuccess: setCodes })
          }}>
            <DialogHeader>
              <DialogTitle>New recovery codes</DialogTitle>
              <DialogDescription>Your current recovery codes stop working.</DialogDescription>
            </DialogHeader>
            <CurrentPassword hasPassword={hasPassword} value={password} onChange={setPassword} error={regenerate.error} fallback="New recovery codes could not be made." />
            <DialogFooter>
              <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
              <Button type="submit" disabled={regenerate.isPending || (hasPassword && !password)}>{regenerate.isPending ? 'Please wait…' : 'Make new codes'}</Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  )
}

function DisableTwoFactorDialog({ onClose }: { onClose: () => void }) {
  const disable = useDisableTwoFactor()
  const hasPassword = useHasPassword()
  const [password, setPassword] = useState('')
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent>
        <form className="grid gap-4" onSubmit={(event) => {
          event.preventDefault()
          disable.mutate(hasPassword ? password : null, {
            onSuccess: () => {
              toast('Two-factor sign-in is off')
              onClose()
            },
          })
        }}>
          <DialogHeader>
            <DialogTitle>Turn off two-factor sign-in?</DialogTitle>
            <DialogDescription>Sign-in no longer asks for a code, and your recovery codes stop working.</DialogDescription>
          </DialogHeader>
          <CurrentPassword hasPassword={hasPassword} value={password} onChange={setPassword} error={disable.error} fallback="Two-factor sign-in could not be turned off." />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
            <Button type="submit" variant="destructive" disabled={disable.isPending || (hasPassword && !password)}>{disable.isPending ? 'Turning off…' : 'Turn off'}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** A read-only value with a copy button. */
function CopyField({ label, value }: { label: string; value: string }) {
  const id = useId()
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <div className="flex items-center gap-2">
        <Input id={id} readOnly value={value} className="font-mono" onFocus={(event) => event.target.select()} />
        <Button type="button" variant="outline" size="icon" aria-label={`Copy ${label.toLowerCase()}`} onClick={() => void copyText(value)}>
          <Copy />
        </Button>
      </div>
    </Field>
  )
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    toast('Copied')
  } catch {
    toast.error('Could not copy. Select and copy it manually.')
  }
}

function downloadCodes(codes: string[]) {
  const text = `Orbit recovery codes\nEach code signs in once in place of an authenticator app code.\n\n${codes.join('\n')}\n`
  const url = URL.createObjectURL(new Blob([text], { type: 'text/plain' }))
  const link = document.createElement('a')
  link.href = url
  link.download = 'orbit-recovery-codes.txt'
  link.click()
  URL.revokeObjectURL(url)
}

function RecoveryCodesStep({ codes, title, onDone }: { codes: string[]; title: string; onDone: () => void }) {
  return (
    <>
      <DialogHeader>
        <DialogTitle>{title}</DialogTitle>
        <DialogDescription>
          Save these recovery codes somewhere safe. Each one signs you in once if you lose your authenticator app. They are shown only now.
        </DialogDescription>
      </DialogHeader>
      <ul aria-label="Recovery codes" className="grid grid-cols-2 gap-x-4 gap-y-1.5 rounded-lg bg-muted p-4 font-mono text-[13px] text-foreground">
        {codes.map((code) => <li key={code}>{code}</li>)}
      </ul>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={() => void copyText(codes.join('\n'))}><Copy />Copy</Button>
        <Button type="button" variant="outline" onClick={() => downloadCodes(codes)}><DocumentDownload />Download</Button>
        <Button type="button" onClick={onDone}>Done</Button>
      </DialogFooter>
    </>
  )
}

function PasskeysCard() {
  const options = useAuthOptions()
  const passkeys = usePasskeys()
  const remove = useDeletePasskey()
  const add = useAddPasskey()
  const [adding, setAdding] = useState(false)
  // Closing the browser prompt is a choice, not a failure.
  const addError = !adding && add.isError && !isPasskeyCancel(add.error) ? problemDetail(add.error, add.error instanceof Error ? add.error.message : 'The passkey could not be added.') : null

  function addPasskey(name: string, password: string | null) {
    add.mutate({
      name,
      password,
      beforePrompt: async () => {
        flushSync(() => setAdding(false))
        await new Promise((resolve) => requestAnimationFrame(resolve))
      },
    }, {
      onSuccess: (passkey) => toast(`${passkey.name} is added`),
    })
  }
  const available = options.data?.passkeys_enabled === true
  const supported = passkeysSupported()
  const list = passkeys.data ?? []

  async function confirmRemove(passkey: PasskeyRecord) {
    const confirmed = await confirmAction({
      title: `Remove ${passkey.name}?`,
      description: 'You can no longer sign in with this passkey.',
      confirmLabel: 'Remove passkey',
      danger: true,
    })
    if (!confirmed) return
    remove.mutate(passkey.id, {
      onError: (error) => toast.error(problemDetail(error, 'The passkey could not be removed.')),
    })
  }

  return (
    <SettingsCard
      title="Passkeys"
      description="Sign in with your fingerprint, face or device screen lock instead of a password."
      actions={available && supported ? <Button type="button" variant="outline" disabled={add.isPending} onClick={() => { add.reset(); setAdding(true) }}>Add passkey</Button> : undefined}
      flush
    >
      <div className="flex flex-col divide-y">
        {options.data && !available ? <SettingsRow className="text-[13px] text-muted-foreground">Passkeys need Orbit to be opened at a domain name.</SettingsRow> : null}
        {available && !supported ? <SettingsRow className="text-[13px] text-muted-foreground">This browser does not support passkeys.</SettingsRow> : null}
        {!adding && add.isPending ? <SettingsRow role="status" className="text-[13px] text-muted-foreground">Confirm the new passkey in your browser…</SettingsRow> : null}
        {addError ? <SettingsRow role="alert" className="text-[13px] text-destructive">{addError}</SettingsRow> : null}
        {passkeys.isPending ? <SettingsRow role="status">Loading passkeys…</SettingsRow> : null}
        {passkeys.isError ? <SettingsRow role="alert">Passkeys could not be loaded. <Button variant="ghost" onClick={() => void passkeys.refetch()}>Retry</Button></SettingsRow> : null}
        {passkeys.isSuccess && list.length === 0 && available ? <SettingsRow className="text-[13px] text-muted-foreground">No passkeys yet.</SettingsRow> : null}
        {list.map((passkey) => (
          <SettingsRow key={passkey.id} className="flex-nowrap" data-passkey={passkey.id}>
            <RowIcon><FingerScan /></RowIcon>
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
              <span className="truncate text-[13px] font-medium text-foreground">{passkey.name}</span>
              <span className="text-xs text-muted-foreground/70">
                Added {ago(passkey.created_at)} · {passkey.last_used_at ? `Last used ${ago(passkey.last_used_at)}` : 'Never used'}
              </span>
            </div>
            <Button type="button" variant="ghost" aria-label={`Remove ${passkey.name}`} disabled={remove.isPending} onClick={() => void confirmRemove(passkey)}>
              Remove
            </Button>
          </SettingsRow>
        ))}
      </div>
      {adding ? <AddPasskeyDialog add={add} onAdd={addPasskey} onClose={() => setAdding(false)} /> : null}
    </SettingsCard>
  )
}

/** Asks for the name and the password (if the account has one). It closes before the browser prompt opens (see `useAddPasskey`). */
function AddPasskeyDialog({ add, onAdd, onClose }: { add: ReturnType<typeof useAddPasskey>; onAdd: (name: string, password: string | null) => void; onClose: () => void }) {
  const hasPassword = useHasPassword()
  const [name, setName] = useState(() => defaultPasskeyName())
  const [password, setPassword] = useState('')
  const nameId = useId()
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent>
        <form className="grid gap-4" onSubmit={(event) => {
          event.preventDefault()
          onAdd(name.trim(), hasPassword ? password : null)
        }}>
          <DialogHeader>
            <DialogTitle>Add a passkey</DialogTitle>
            <DialogDescription>Your browser asks you to confirm with your fingerprint, face, screen lock or security key.</DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor={nameId}>Name</FieldLabel>
              <Input id={nameId} required maxLength={64} value={name} onChange={(event) => setName(event.target.value)} />
              <FieldDescription>So you can tell your passkeys apart, e.g. "MacBook" or "YubiKey".</FieldDescription>
            </Field>
            {/* Only the password check fails while the dialog is open. */}
            <CurrentPassword hasPassword={hasPassword} value={password} onChange={setPassword} error={add.error} fallback="The passkey could not be added." />
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
            <Button type="submit" disabled={add.isPending || (hasPassword && !password) || !name.trim()}>{add.isPending ? 'Checking…' : 'Add passkey'}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** A connected account is a new way into the account, so an account with a password confirms it first. */
function ConnectProviderDialog({ provider, onClose }: { provider: Provider; onClose: () => void }) {
  const connect = useStartProviderSignIn()
  const [password, setPassword] = useState('')
  const name = PROVIDER_NAMES[provider]
  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent>
        <form className="grid gap-4" onSubmit={(event) => {
          event.preventDefault()
          connect.mutate({ provider, intent: 'connect', returnTo: SECURITY_PATH, password })
        }}>
          <DialogHeader>
            <DialogTitle>Connect {name}</DialogTitle>
            <DialogDescription>Confirm your password, then sign in with {name}.</DialogDescription>
          </DialogHeader>
          <CurrentPassword hasPassword value={password} onChange={setPassword} error={connect.error} fallback="The connection could not start." />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
            <Button type="submit" disabled={connect.isPending || connect.isSuccess || !password}>{connect.isPending ? 'Please wait…' : 'Continue'}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function ConnectedAccountsCard() {
  const options = useAuthOptions()
  const accounts = useConnectedAccounts()
  const hasPassword = useHasPassword()
  const connect = useStartProviderSignIn()
  const disconnect = useDisconnectAccount()
  const [confirming, setConfirming] = useState<Provider | null>(null)
  const configured = options.data?.providers ?? []
  const connected = accounts.data ?? []
  // An account stays listed after an admin removes its provider, so it can still be disconnected.
  const providers = [...configured, ...connected.map((account) => account.provider).filter((provider) => !configured.includes(provider))]
  if (providers.length === 0) return null

  async function confirmDisconnect(account: ConnectedAccount) {
    const name = PROVIDER_NAMES[account.provider]
    const confirmed = await confirmAction({
      title: `Disconnect ${name}?`,
      description: `You can no longer sign in with this ${name} account.`,
      confirmLabel: 'Disconnect',
      danger: true,
    })
    if (!confirmed) return
    connect.reset()
    disconnect.mutate(account.provider, { onSuccess: () => toast(`${name} is disconnected`) })
  }

  function connectProvider(provider: Provider) {
    disconnect.reset()
    if (hasPassword) setConfirming(provider)
    else connect.mutate({ provider, intent: 'connect', returnTo: SECURITY_PATH })
  }

  return (
    <SettingsCard title="Connected accounts" description="Sign in to Orbit with an account you already have." flush>
      <div className="flex flex-col divide-y">
        {disconnect.isError ? <SettingsRow role="alert" className="text-[13px] text-destructive">{problemDetail(disconnect.error, 'The account could not be disconnected.')}</SettingsRow> : null}
        {connect.error instanceof ApiProblem && connect.error.code === 'reauthentication_required'
          ? <SettingsRow><ConfirmWithProvider returnTo={SECURITY_PATH} /></SettingsRow>
          : connect.isError ? <SettingsRow role="alert" className="text-[13px] text-destructive">{problemDetail(connect.error, 'The connection could not start.')}</SettingsRow> : null}
        {accounts.isError ? <SettingsRow role="alert">Connected accounts could not be loaded. <Button variant="ghost" onClick={() => void accounts.refetch()}>Retry</Button></SettingsRow> : null}
        {providers.map((provider) => {
          const account = connected.find((item) => item.provider === provider)
          const name = PROVIDER_NAMES[provider]
          return (
            <SettingsRow key={provider} className="flex-nowrap" data-provider={provider}>
              <RowIcon className="text-foreground"><ProviderMark provider={provider} /></RowIcon>
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="text-[13px] font-medium text-foreground">{name}</span>
                <span className="truncate text-xs text-muted-foreground/70">
                  {account ? `${account.email ? `${account.email} · ` : ''}Connected ${ago(account.created_at)}` : 'Not connected'}
                </span>
              </div>
              {account ? (
                <Button type="button" variant="ghost" aria-label={`Disconnect ${name}`} disabled={disconnect.isPending} onClick={() => void confirmDisconnect(account)}>Disconnect</Button>
              ) : accounts.isSuccess && configured.includes(provider) ? (
                <Button type="button" variant="outline" aria-label={`Connect ${name}`} disabled={connect.isPending || connect.isSuccess} onClick={() => connectProvider(provider)}>Connect</Button>
              ) : null}
            </SettingsRow>
          )
        })}
      </div>
      {confirming ? <ConnectProviderDialog provider={confirming} onClose={() => setConfirming(null)} /> : null}
    </SettingsCard>
  )
}
