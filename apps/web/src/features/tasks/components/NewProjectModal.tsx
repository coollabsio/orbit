import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { DialogFooter, DialogClose } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Modal } from '@/components/common/Modal'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import type { Project } from '@/features/tasks/api/models'
import { projectDraft } from '@/features/tasks/api/projectDraft'
import { useCreateProject } from '@/features/tasks/api/projects'

export function NewProjectModal({ onClose, onCreated }: { onClose: () => void; onCreated: (project: Project) => void }) {
  const { workspace } = useWorkspace()
  const createProject = useCreateProject(workspace.id)
  const [name, setName] = useState('')
  const inputId = useId()

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    const projectName = name.trim()
    if (!projectName || createProject.isPending) return
    try {
      onCreated(await createProject.mutateAsync(projectDraft(projectName)))
    } catch {
      // Keep the modal and draft open so the user can retry.
    }
  }

  return (
    <Modal title="New project" onClose={onClose} className="sm:max-w-md">
      <form className="grid gap-4" onSubmit={(event) => void submit(event)}>
        <Field>
          <FieldLabel htmlFor={inputId}>Project name <span className="font-semibold text-primary">*</span></FieldLabel>
          <Input id={inputId} autoFocus required value={name} disabled={createProject.isPending} onChange={(event) => setName(event.target.value)} />
        </Field>
        {createProject.isError ? <p role="alert" className="text-xs text-destructive">Project creation failed. Try again.</p> : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" disabled={createProject.isPending} />}>Cancel</DialogClose>
          <Button type="submit" disabled={!name.trim() || createProject.isPending}>
            {createProject.isPending ? 'Creating…' : 'Create project'}
          </Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
