import { useState } from 'react'
import { View } from 'react-native'

import { confirmsEmail, deleteAccount, ownedWorkspaces } from '../../data/account-api'
import { withReauthentication } from '../../data/sign-in-provider'
import { useAuthOptions, useConfirmIdentity } from '../../hooks/account'
import { errorMessage, useSignedIn } from '../../session-context'
import { Button, Input, Text } from '../../ui'
import { Dialog } from './Dialog'
import { Note } from './Shell'

/** The web's Delete account dialog. It needs no workspace, so the "No workspaces" screen opens it too. */
export function DeleteAccountDialog({ onClose }: { onClose: () => void }) {
  const { client, user, signOut } = useSignedIn()
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
