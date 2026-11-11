import { useQuery } from '@tanstack/react-query'
import { router } from 'expo-router'
import { useState } from 'react'
import { View } from 'react-native'

import { Dialog } from '../../../components/settings/Dialog'
import { FieldLabel, Note, SettingsCard, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { billingKey, workspaceBilling } from '../../../data/billing'
import { useAuthOptions, useDeleteWorkspace } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { Button, Input, Text } from '../../../ui'

/** The web's Danger zone: the owner deletes the workspace after typing its name. */
export default function DangerZoneScreen() {
  const { workspace } = useOrbit()
  const canDelete = workspace.permissions.includes('workspace.delete')
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV}>
      {canDelete ? (
        <DeleteWorkspace key={workspace.id} />
      ) : (
        <SettingsCard title="Danger zone">
          <Text className="text-[14px] leading-[21px]">Only the workspace owner can delete this workspace.</Text>
        </SettingsCard>
      )}
    </SettingsShell>
  )
}

function DeleteWorkspace() {
  const { client, workspace, workspaces } = useOrbit()
  const deletion = useDeleteWorkspace()
  const cloud = useAuthOptions().data?.billing_enabled === true
  // the server cancels a subscription that has not ended when the workspace is deleted
  const status = useQuery({ queryKey: billingKey(workspace.id), queryFn: () => workspaceBilling(client, workspace.id), enabled: cloud }).data?.status
  const subscribed = status != null && status !== 'canceled' && status !== 'incomplete_expired'
  const [open, setOpen] = useState(false)
  const [confirmation, setConfirmation] = useState('')
  const close = () => {
    if (!deletion.isPending) setOpen(false)
  }
  const submit = () => {
    if (confirmation !== workspace.name || deletion.isPending) return
    deletion.mutate(undefined, {
      onSuccess: () => {
        setOpen(false)
        // the workspace list drops it and the app opens the next one, as Leave workspace does
        router.replace('/settings')
      },
    })
  }

  return (
    <>
      <SettingsCard title="Danger zone" description="Delete this workspace and remove access for all members." danger>
        <View className="items-start">
          <Button
            label="Delete workspace"
            variant="destructive"
            onPress={() => {
              setConfirmation('')
              deletion.reset()
              setOpen(true)
            }}
          />
        </View>
      </SettingsCard>
      <Dialog
        visible={open}
        title="Delete workspace?"
        description={`Deleting "${workspace.name}" removes access to its projects and tasks for everyone.`}
        onClose={close}
        footer={
          <>
            <Button label="Cancel" variant="ghost" disabled={deletion.isPending} onPress={close} />
            <Button label={deletion.isPending ? 'Deleting…' : 'Delete workspace'} variant="destructive" disabled={confirmation !== workspace.name || deletion.isPending} onPress={submit} />
          </>
        }
      >
        {workspaces.length === 1 ? <Note error>This is your last workspace. After deletion, you will have no workspace access.</Note> : null}
        {subscribed ? (
          <Note error>
            Deleting cancels the subscription at once, with no refund for the rest of the billing period. The workspace stays in the trash for 30 days; restoring it does not restore the subscription.
          </Note>
        ) : null}
        {/* the web's field: label, then the hint, then the input */}
        <View className="gap-2">
          <FieldLabel>Confirm workspace name</FieldLabel>
          <Text className="text-[14px] leading-[21px] text-muted-foreground">
            Type <Text className="font-noto-semibold text-[14px] leading-[21px] text-foreground">{workspace.name}</Text> to confirm deletion.
          </Text>
          <Input
            accessibilityLabel="Confirm workspace name"
            value={confirmation}
            onChangeText={setConfirmation}
            editable={!deletion.isPending}
            autoCapitalize="none"
            autoCorrect={false}
            autoComplete="off"
            returnKeyType="done"
            onSubmitEditing={submit}
          />
        </View>
        {deletion.isError ? <Note error>Workspace deletion failed. Try again.</Note> : null}
      </Dialog>
    </>
  )
}
