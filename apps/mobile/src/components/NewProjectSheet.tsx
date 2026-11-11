import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { createProject, projectDraft, projects, type ProjectRecord } from '../data/orbit'
import { keys } from '../data/keys'
import { useOrbit } from '../orbit-context'
import { Button, Input } from '../ui'
import { ChoiceGroup } from './chat/Sheet'
import { Dialog } from './settings/Dialog'

/** The web's `NewProjectModal`: a name and who sees it, then Create project. A failure keeps the dialog and the name for a retry. */
export function NewProjectSheet({ visible, onClose, onCreated }: { visible: boolean; onClose: () => void; onCreated: (project: ProjectRecord) => void }) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const [name, setName] = useState('')
  const [access, setAccess] = useState<'workspace' | 'private'>('workspace')
  const create = useMutation({
    mutationFn: async ({ projectName, isPrivate }: { projectName: string; isPrivate: boolean }) => {
      // A fresh list, so the key skips the keys of projects that other members made since the last load.
      const existing = await projects(client, workspaceId)
      return createProject(client, workspaceId, { ...projectDraft(projectName, existing.map((project) => project.key)), private: isPrivate })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.projects(workspaceId) }),
  })
  const close = () => {
    if (create.isPending) return
    setName('')
    setAccess('workspace')
    create.reset()
    onClose()
  }
  const submit = () => {
    const projectName = name.trim()
    if (!projectName || create.isPending) return
    create.mutate({ projectName, isPrivate: access === 'private' }, {
      onSuccess: (project) => {
        setName('')
        setAccess('workspace')
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
      <ChoiceGroup
        label="Access"
        value={access}
        onChange={setAccess}
        options={[
          { value: 'workspace', label: 'Workspace', description: 'Every member of the workspace sees it.', icon: 'people' },
          { value: 'private', label: 'Private', description: 'Only the people you add, and workspace owners and admins.', icon: 'lock' },
        ]}
      />
    </Dialog>
  )
}
