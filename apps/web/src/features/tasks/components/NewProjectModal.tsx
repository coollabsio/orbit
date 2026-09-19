import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Modal } from '../../../components/ui/Modal'
import { useWorkspace } from '../../workspaces/workspaceContext'
import type { Project } from '../api/models'
import { projectDraft } from '../api/projectDraft'
import { useCreateProject } from '../api/projects'

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
    <Modal title="New project" onClose={onClose} maxWidth={448}>
      <form onSubmit={(event) => void submit(event)}>
        <div className="w-full min-w-0">
          <Label className="mb-1.5 flex h-4 items-center gap-1 text-[13px] leading-4 font-medium text-muted-foreground" htmlFor={inputId}>Project name <span className="font-semibold text-primary">*</span></Label>
          <Input id={inputId} autoFocus required value={name} disabled={createProject.isPending} onChange={(event) => setName(event.target.value)} />
        </div>
        {createProject.isError ? <p role="alert" className="text-xs text-destructive">Project creation failed. Try again.</p> : null}
        <div className="mt-4 flex flex-wrap items-center justify-end gap-2 border-t border-border pt-4">
          <Button type="button" variant="outline" disabled={createProject.isPending} onClick={onClose}>Cancel</Button>
          <Button type="submit" disabled={!name.trim() || createProject.isPending}>
            {createProject.isPending ? 'Creating…' : 'Create project'}
          </Button>
        </div>
      </form>
    </Modal>
  )
}
