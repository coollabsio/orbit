import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { createProject, projectDraft, type ProjectRecord } from '../data/orbit'
import { keys } from '../data/keys'
import { useOrbit } from '../orbit-context'
import { Button, Input } from '../ui'
import { Dialog } from './settings/Dialog'

/** The web's `NewProjectModal`: a name, then Create project. A failure keeps the dialog and the name for a retry. */
export function NewProjectSheet({ visible, onClose, onCreated }: { visible: boolean; onClose: () => void; onCreated: (project: ProjectRecord) => void }) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const [name, setName] = useState('')
  const create = useMutation({
    mutationFn: (projectName: string) => createProject(client, workspaceId, projectDraft(projectName)),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.projects(workspaceId) }),
  })
  const close = () => {
    if (create.isPending) return
    setName('')
    create.reset()
    onClose()
  }
  const submit = () => {
    const projectName = name.trim()
    if (!projectName || create.isPending) return
    create.mutate(projectName, {
      onSuccess: (project) => {
        setName('')
        create.reset()
        onCreated(project)
      },
    })
  }
  return (
    <Dialog
      visible={visible}
      title="New project"
      onClose={close}
      footer={
        <>
          <Button label="Cancel" variant="outline" disabled={create.isPending} onPress={close} />
          <Button label={create.isPending ? 'Creating…' : 'Create project'} disabled={!name.trim() || create.isPending} onPress={submit} />
        </>
      }
    >
      <Input
        label="Project name"
        required
        autoFocus
        value={name}
        editable={!create.isPending}
        onChangeText={setName}
        returnKeyType="done"
        onSubmitEditing={submit}
        error={create.isError ? 'Project creation failed. Try again.' : undefined}
      />
    </Dialog>
  )
}
