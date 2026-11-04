import { useState } from 'react'
import { View } from 'react-native'

import { useSignedIn } from '../session-context'
import { Button, EmptyState, Screen } from '../ui'
import { DeleteAccountDialog } from './settings/DeleteAccountDialog'

/**
 * What a signed-in person with no workspace sees (the last one is gone, or they left it). The app has no onboarding,
 * so they sign out or delete the account here; the root account cannot be deleted.
 */
export function NoWorkspace() {
  const { user, signOut } = useSignedIn()
  const [deleting, setDeleting] = useState(false)
  return (
    <Screen centered>
      <EmptyState
        icon="home"
        title="No workspaces"
        description="Ask an admin to invite you, or create a workspace in Orbit on the web."
        action={
          <View className="items-center gap-2">
            <Button label="Sign out" variant="outline" onPress={() => void signOut()} />
            {user.root ? null : <Button label="Delete account" variant="destructive" onPress={() => setDeleting(true)} />}
          </View>
        }
      />
      {deleting ? <DeleteAccountDialog onClose={() => setDeleting(false)} /> : null}
    </Screen>
  )
}
