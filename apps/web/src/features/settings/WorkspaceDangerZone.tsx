import { useCallback, useId, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router'
import { Modal } from '../../components/ui/Modal'
import { useDeleteWorkspace } from '../workspaces/api'
import { useWorkspace } from '../workspaces/workspaceContext'
import { SettingsCard } from './SettingsCard'

export function WorkspaceDangerZone() {
  const { workspace, workspaces } = useWorkspace()
  const navigate = useNavigate()
  const deletion = useDeleteWorkspace(workspace.id)
  const [open, setOpen] = useState(false)
  const [confirmation, setConfirmation] = useState('')
  const inputId = useId()
  const deleting = useRef(false)
  const close = useCallback(() => {
    if (!deleting.current) setOpen(false)
  }, [])

  if (workspace.role !== 'owner') return null

  async function deleteConfirmed() {
    if (confirmation !== workspace.name || deleting.current) return
    deleting.current = true
    try {
      await deletion.mutateAsync(workspace.version)
      setOpen(false)
      const next = workspaces.find((item) => item.id !== workspace.id)
      if (next) navigate(`/settings?${new URLSearchParams({ workspace: next.id })}`, { replace: true })
    } catch {
      // Keep the confirmation open and show the mutation error.
    } finally {
      deleting.current = false
    }
  }

  return <>
    <SettingsCard className="settings-danger-zone" title="Danger zone" description="Delete this workspace and remove access for all members.">
      <button type="button" className="button button-danger" onClick={() => { setConfirmation(''); deletion.reset(); setOpen(true) }}>Delete workspace</button>
    </SettingsCard>
    {open ? createPortal(
      <Modal title="Delete workspace?" description={`Deleting "${workspace.name}" removes access to its projects and tasks for everyone.`} onClose={close} maxWidth={480}>
        <form onSubmit={(event) => { event.preventDefault(); void deleteConfirmed() }}>
          {workspaces.length === 1 ? <p className="text-danger workspace-delete-warning">This is your last workspace. After deletion, you will have no workspace access.</p> : null}
          <div className="settings-field">
            <label className="field-label" htmlFor={inputId}>Confirm workspace name</label>
            <p className="text-xs">Type <strong>{workspace.name}</strong> to confirm deletion.</p>
            <input id={inputId} className="input" autoComplete="off" required value={confirmation} readOnly={deletion.isPending} onChange={(event) => setConfirmation(event.target.value)} />
          </div>
          {deletion.isError ? <p role="alert" className="text-danger workspace-delete-warning">Workspace deletion failed. Try again, or refresh the page if the workspace has changed.</p> : null}
          <div className="modal-footer">
            <button type="button" className="button button-ghost" disabled={deletion.isPending} onClick={close}>Cancel</button>
            <button type="submit" className="button button-danger" disabled={confirmation !== workspace.name} aria-disabled={deletion.isPending || undefined}>{deletion.isPending ? 'Deleting…' : 'Delete workspace'}</button>
          </div>
        </form>
      </Modal>, document.body,
    ) : null}
  </>
}
