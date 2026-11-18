import { ApiProblem } from '@orbit/api/problem'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { View } from 'react-native'

import { accountTrial, accountTrialKey, newWorkspaceTrialNote } from '../data/billing'
import { keys } from '../data/keys'
import { createWorkspace, type WorkspaceRecord } from '../data/orbit'
import { useAuthOptions } from '../hooks/account'
import { saveWorkspaceId } from '../session'
import { useSignedIn } from '../session-context'
import { Button, Input } from '../ui'

/**
 * The web's onboarding workspace step and the switcher's "Create workspace" form: a name, the trial note on Orbit
 * Cloud, then Create workspace. Any account may try; the server refuses one over the admin's owned-workspace limit,
 * and its message shows under the field. It renders without a workspace too (`NoWorkspace`), so it reads the session,
 * not `useOrbit()`. A failure keeps the name for a retry.
 */
export function NewWorkspaceForm({ onCreated, onCancel }: { /** Inside a workspace: `selectWorkspace`. */ onCreated?: (workspace: WorkspaceRecord) => void; /** Shows Cancel. */ onCancel?: () => void }) {
  const { client } = useSignedIn()
  const queryClient = useQueryClient()
  const [name, setName] = useState('')
  const billing = useAuthOptions().data?.billing_enabled ?? false
  const trial = useQuery({ queryKey: accountTrialKey, queryFn: () => accountTrial(client), enabled: billing })
  const trialNote = billing ? newWorkspaceTrialNote(trial.data, Date.now()) : null
  const create = useMutation({
    mutationFn: (workspaceName: string) => createWorkspace(client, workspaceName),
    onSuccess: (workspace) => {
      // Here, not in `onCreated`: without a workspace the new list replaces this form, and a call's own callback does
      // not run once its component is gone.
      void saveWorkspaceId(workspace.id)
      queryClient.setQueryData<WorkspaceRecord[]>(keys.workspaces, (items) => (items ? [...items, workspace] : [workspace]))
      // The first workspace starts the account's trial.
      void queryClient.invalidateQueries({ queryKey: accountTrialKey })
    },
  })
  const submit = () => {
    const workspaceName = name.trim()
    if (!workspaceName || create.isPending) return
    create.mutate(workspaceName, { onSuccess: onCreated })
  }
  return (
    <View className="gap-4">
      <Input
        label="Workspace name"
        required
        autoFocus
        placeholder="Acme Inc."
        help="A workspace is where your team works together. It holds your projects, tasks, documents and chat. You can invite people and add more workspaces later."
        value={name}
        editable={!create.isPending}
        onChangeText={setName}
        returnKeyType="done"
        onSubmitEditing={submit}
        description={trialNote ?? undefined}
        error={create.isError ? (create.error instanceof ApiProblem ? create.error.detail : 'Workspace creation failed. Try again.') : undefined}
      />
      <Button label={create.isPending ? 'Creating…' : 'Create workspace'} block disabled={!name.trim() || create.isPending} onPress={submit} />
      {onCancel ? <Button label="Cancel" variant="ghost" block disabled={create.isPending} onPress={onCancel} /> : null}
    </View>
  )
}
