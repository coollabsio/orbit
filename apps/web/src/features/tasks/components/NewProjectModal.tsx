import { useId, useState } from 'react'
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
        <div className="settings-field">
          <label className="field-label" htmlFor={inputId}>Project name <span className="field-required">*</span></label>
          <input id={inputId} className="input" autoFocus required value={name} disabled={createProject.isPending} onChange={(event) => setName(event.target.value)} />
        </div>
        {createProject.isError ? <p role="alert" className="text-danger text-xs">Project creation failed. Try again.</p> : null}
        <div className="modal-footer">
          <button type="button" className="button" disabled={createProject.isPending} onClick={onClose}>Cancel</button>
          <button type="submit" className="button button-primary" disabled={!name.trim() || createProject.isPending}>
            {createProject.isPending ? 'Creating…' : 'Create project'}
          </button>
        </div>
      </form>
    </Modal>
  )
}
