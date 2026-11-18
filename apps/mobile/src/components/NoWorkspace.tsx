import { useState } from 'react'
import { View } from 'react-native'

import { useSignedIn } from '../session-context'
import { Button, Screen, Text } from '../ui'
import { NewWorkspaceForm } from './NewWorkspaceForm'
import { DeleteAccountDialog } from './settings/DeleteAccountDialog'

/**
 * What a signed-in person with no workspace sees (a new account, the root user after setup, or the last workspace is
 * gone): the web's onboarding workspace step, in the sign-in card's look. The new workspace fills the workspaces query,
 * so `OrbitProvider` opens it. They can also sign out or delete the account here; the root account cannot be deleted.
 */
export function NoWorkspace() {
  const { user, signOut } = useSignedIn()
  const [deleting, setDeleting] = useState(false)
  return (
    <Screen centered scroll>
      <View className="w-full max-w-115 self-center rounded-xl border border-border bg-card shadow-lg">
        <View className="gap-1 px-6 pt-6 pb-5">
          <Text className="font-noto-semibold text-[20px] leading-[28px]">Create your workspace</Text>
          <Text className="text-[13px] leading-[19.5px] text-muted-foreground">You have no workspace. Create one, or ask an admin to invite you.</Text>
        </View>
        <View className="-mx-px -mb-px rounded-xl border border-border bg-background/40 p-6">
          <NewWorkspaceForm />
        </View>
      </View>
      <View className="items-center gap-2">
        <Button label="Sign out" variant="ghost" onPress={() => void signOut()} />
        {user.root ? null : <Button label="Delete account" variant="ghost" onPress={() => setDeleting(true)} />}
      </View>
      {deleting ? <DeleteAccountDialog onClose={() => setDeleting(false)} /> : null}
    </Screen>
  )
}
