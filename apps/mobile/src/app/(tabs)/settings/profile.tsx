import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
import { View } from 'react-native'

import { Dialog } from '../../../components/settings/Dialog'
import { ProfilePicture } from '../../../components/settings/ProfilePicture'
import { ACCOUNT_NAV, Note, SettingsCard, SettingsShell } from '../../../components/settings/Shell'
import { confirmsEmail, deleteAccount, ownedWorkspaces, passwordProblem, requestEmailChange } from '../../../data/account-api'
import * as api from '../../../data/orbit'
import { phoneValid, profileChanges, profileFormOf, type ProfileForm } from '../../../data/profile'
import { withReauthentication } from '../../../data/sign-in-provider'
import { pickImage, useAuthOptions, useChangePassword, useConfirmIdentity, useSetProfilePicture } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { errorMessage } from '../../../session-context'
import { Button, Input, Text } from '../../../ui'

/** The server refuses a bigger picture. */
const MAX_PICTURE_BYTES = 512 * 1024
const BIO_LIMIT = 500
/** The bio counter shows from this many characters before the limit. */
const BIO_COUNTER_FROM = 100

/** The web's Profile page: the picture, the details other members see, the email, the password and deleting the account. */
export default function ProfileScreen() {
  const { client, user, setUser } = useOrbit()
  const saved = profileFormOf(user)
  const [form, setForm] = useState<ProfileForm>(saved)
  const set = (part: Partial<ProfileForm>) => setForm((current) => ({ ...current, ...part }))
  const changes = profileChanges(user, form)
  const phoneInvalid = form.phone.trim() !== '' && !phoneValid(form.phone.trim())
  const bioLeft = BIO_LIMIT - form.bio.length

  const save = useMutation({
    mutationFn: (body: api.UpdateMeBody) => api.updateProfile(client, body),
    onSuccess: (updated) => {
      setUser(updated)
      setForm(profileFormOf(updated))
    },
  })

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV}>
      <SettingsCard title="Profile details" description="Your picture, name and what the people in your workspaces see of you.">
        <PictureField />
        <View className="mt-4 gap-4">
          <Input label="Name" required value={form.name} onChangeText={(name) => set({ name })} autoComplete="name" />
          <EmailField />
          <Input label="Title" value={form.title} onChangeText={(title) => set({ title })} placeholder="Product designer" maxLength={100} />
          <Input label="Pronouns" value={form.pronouns} onChangeText={(pronouns) => set({ pronouns })} placeholder="she/her" maxLength={40} />
          <Input
            label="Phone number"
            value={form.phone}
            onChangeText={(phone) => set({ phone })}
            placeholder="+1 555 010 0000"
            keyboardType="phone-pad"
            autoComplete="tel"
            description="The people in your workspaces can see it."
            error={phoneInvalid ? 'Use digits, spaces and + - ( ) . only.' : undefined}
          />
          <Input label="Time zone" value={form.timezone} onChangeText={(timezone) => set({ timezone })} placeholder="Europe/Budapest" autoCapitalize="none" autoCorrect={false} />
          <View className="gap-1">
            <Input label="Bio" value={form.bio} onChangeText={(bio) => set({ bio })} multiline maxLength={BIO_LIMIT} />
            {bioLeft <= BIO_COUNTER_FROM ? <Text className="text-[12px] text-right text-muted-foreground">{`${form.bio.length} / ${BIO_LIMIT}`}</Text> : null}
          </View>
        </View>
        {save.isError ? <View className="mt-3"><Note error>{errorMessage(save.error)}</Note></View> : null}
        {changes ? (
          <View className="mt-4 flex-row gap-2 justify-end">
            <Button label="Discard" variant="outline" onPress={() => setForm(saved)} />
            <Button label="Save" loading={save.isPending} disabled={phoneInvalid} onPress={() => save.mutate(changes)} />
          </View>
        ) : null}
      </SettingsCard>

      <PasswordCard />
      <DeleteAccountCard />
    </SettingsShell>
  )
}

function PictureField() {
  const { user } = useOrbit()
  const setPicture = useSetProfilePicture()
  const [pickError, setPickError] = useState<string | null>(null)
  const choose = () => {
    setPickError(null)
    setPicture.reset()
    pickImage({ square: true, maxBytes: MAX_PICTURE_BYTES }).then(
      (file) => file && setPicture.mutate(file),
      (error: unknown) => setPickError(errorMessage(error)),
    )
  }
  const error = pickError ?? (setPicture.isError ? errorMessage(setPicture.error) : null)
  return (
    <View className="flex-row items-center gap-4 flex-wrap">
      <ProfilePicture id={user.id} name={user.display_name} url={user.avatar_url} size={64} />
      <View className="flex-1 min-w-45 gap-2">
        <View className="flex-row gap-2">
          <Button label={setPicture.isPending ? 'Saving…' : user.avatar_url ? 'Change picture' : 'Upload picture'} variant="outline" disabled={setPicture.isPending} onPress={choose} />
          {user.avatar_url ? <Button label="Remove" variant="ghost" disabled={setPicture.isPending} onPress={() => setPicture.mutate(null)} /> : null}
        </View>
        <Note error={error !== null}>{error ?? 'Cropped to a square. People in your workspaces see it.'}</Note>
      </View>
    </View>
  )
}

function PasswordCard() {
  const change = useChangePassword()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirm, setConfirm] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  const [done, setDone] = useState(false)
  const submit = () => {
    const found = passwordProblem(current, next, confirm)
    setProblem(found)
    setDone(false)
    if (found) return
    change.mutate(
      { current, next },
      {
        onSuccess: () => {
          setCurrent('')
          setNext('')
          setConfirm('')
          setDone(true)
        },
      },
    )
  }
  const alert = problem ?? (change.isError ? errorMessage(change.error) : null)
  return (
    <SettingsCard title="Password" description="Other devices will be signed out." actions={<Button label={change.isPending ? 'Changing…' : 'Change password'} variant="outline" disabled={change.isPending} onPress={submit} />}>
      <View className="gap-4">
        <Input label="Current password" required value={current} onChangeText={setCurrent} secureTextEntry autoComplete="current-password" autoCapitalize="none" />
        <Input label="New password" required value={next} onChangeText={setNext} secureTextEntry autoComplete="new-password" autoCapitalize="none" />
        <Input label="Confirm new password" required value={confirm} onChangeText={setConfirm} secureTextEntry autoComplete="new-password" autoCapitalize="none" returnKeyType="done" onSubmitEditing={submit} />
        {alert ? <Note error>{alert}</Note> : done ? <Note>Your password is changed. Your other devices are signed out.</Note> : null}
      </View>
    </SettingsCard>
  )
}

/** The email, read-only, with Change when the server can email the confirmation link. */
function EmailField() {
  const { user } = useOrbit()
  const options = useAuthOptions()
  const [changing, setChanging] = useState(false)
  const emailEnabled = options.data?.email_enabled === true
  return (
    <View className="gap-2">
      <Input
        label="Email"
        value={user.email}
        editable={false}
        description={options.data && !emailEnabled ? 'This Orbit cannot send email, so the email cannot be changed. Ask an administrator.' : undefined}
      />
      {emailEnabled ? <View className="items-start"><Button label="Change" variant="outline" onPress={() => setChanging(true)} /></View> : null}
      {changing ? <EmailChangeDialog onClose={() => setChanging(false)} /> : null}
    </View>
  )
}

/**
 * The web's email change dialog. The link goes to the new address and opens Orbit on the web; the email changes only
 * then, so the dialog ends on "Check your inbox". An account without a password confirms with its provider when asked.
 */
function EmailChangeDialog({ onClose }: { onClose: () => void }) {
  const { client, user } = useOrbit()
  const confirmIdentity = useConfirmIdentity()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [sentTo, setSentTo] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const ready = email.trim() !== '' && (!user.has_password || password !== '')

  const submit = () => {
    if (!ready || busy) return
    const next = email.trim()
    setBusy(true)
    setError(null)
    const send = () => requestEmailChange(client, next, user.has_password ? password : undefined)
    ;(user.has_password ? send() : withReauthentication(send, confirmIdentity))
      .then((sent) => {
        // `null`: the person cancelled the provider confirmation
        if (sent !== null) setSentTo(next)
      })
      .catch((reason: unknown) => setError(errorMessage(reason)))
      .finally(() => setBusy(false))
  }

  if (sentTo) {
    return (
      <Dialog
        visible
        title="Check your inbox"
        description={`We sent a link to ${sentTo}. Open it within 60 minutes to finish the change. Until then, you sign in with ${user.email}.`}
        onClose={onClose}
        footer={<Button label="Done" onPress={onClose} />}
      />
    )
  }
  return (
    <Dialog
      visible
      title="Change your email"
      description="We send a link to the new address. Your email changes when you open it."
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={busy ? 'Sending…' : 'Send link'} disabled={!ready || busy} onPress={submit} />
        </>
      }
    >
      <Input label="New email" value={email} onChangeText={setEmail} keyboardType="email-address" autoCapitalize="none" autoCorrect={false} autoComplete="email" autoFocus description={`Now ${user.email}.`} returnKeyType={user.has_password ? 'next' : 'done'} onSubmitEditing={submit} />
      {user.has_password ? <Input label="Current password" value={password} onChangeText={setPassword} secureTextEntry autoComplete="current-password" autoCapitalize="none" returnKeyType="done" onSubmitEditing={submit} /> : null}
      {error ? <Note error>{error}</Note> : null}
    </Dialog>
  )
}

/** The web's Delete account card. The root account cannot be deleted, so it only says so. */
function DeleteAccountCard() {
  const { user } = useOrbit()
  const [open, setOpen] = useState(false)
  if (user.root) return <SettingsCard title="Delete account" description="The root account cannot be deleted." />
  return (
    <SettingsCard title="Delete account" description={'Delete your account for good. What you wrote stays, without your name.'}>
      <View className="items-start">
        <Button label="Delete account" variant="destructive" onPress={() => setOpen(true)} />
      </View>
      {open ? <DeleteAccountDialog onClose={() => setOpen(false)} /> : null}
    </SettingsCard>
  )
}

function DeleteAccountDialog({ onClose }: { onClose: () => void }) {
  const { client, user, signOut } = useOrbit()
  const options = useAuthOptions()
  const confirmIdentity = useConfirmIdentity()
  const [typed, setTyped] = useState('')
  const [password, setPassword] = useState('')
  const [failure, setFailure] = useState<unknown>(null)
  const [busy, setBusy] = useState(false)
  const ready = confirmsEmail(typed, user.email) && (!user.has_password || password !== '')
  const owned = ownedWorkspaces(failure)

  const submit = () => {
    if (!ready || busy) return
    setBusy(true)
    setFailure(null)
    const remove = () => deleteAccount(client, typed.trim(), user.has_password ? password : undefined)
    ;(user.has_password ? remove() : withReauthentication(remove, confirmIdentity))
      .then(async (deleted) => {
        setBusy(false)
        // signing out shows the sign-in screen
        if (deleted !== null) await signOut()
      })
      .catch((reason: unknown) => {
        setBusy(false)
        setFailure(reason)
      })
  }

  return (
    <Dialog
      visible
      title="Delete your account?"
      description="You are signed out everywhere and cannot sign in again. This cannot be undone."
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={busy ? 'Deleting…' : 'Delete account'} variant="destructive" disabled={!ready || busy} onPress={submit} />
        </>
      }
    >
      <Input label="Confirm your email" description={`Type ${user.email} to confirm.`} value={typed} onChangeText={setTyped} keyboardType="email-address" autoCapitalize="none" autoCorrect={false} autoComplete="off" />
      {user.has_password ? <Input label="Current password" value={password} onChangeText={setPassword} secureTextEntry autoComplete="current-password" autoCapitalize="none" returnKeyType="done" onSubmitEditing={submit} /> : null}
      {owned ? (
        <View className="gap-2">
          <Note error>{`You own ${owned.length === 1 ? 'this workspace' : 'these workspaces'}:`}</Note>
          <View className="gap-1 px-4 py-3 rounded-lg bg-muted">
            {owned.map((workspace) => <Text key={workspace.id} className="font-noto-medium text-[13px]">{workspace.name}</Text>)}
          </View>
          <Note>
            {options.data?.billing_enabled
              ? 'Delete them first in Workspace settings → Danger zone on the web. Ownership transfer is not available on Orbit Cloud.'
              : 'Transfer them to another member (Workspace settings → Members) or delete them (Workspace settings → Danger zone on the web) first.'}
          </Note>
        </View>
      ) : failure ? <Note error>{errorMessage(failure)}</Note> : null}
    </Dialog>
  )
}
