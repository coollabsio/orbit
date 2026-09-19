import { useCallback, useId, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router'
import { Modal } from '../../components/ui/Modal'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
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
    <SettingsCard className="shadow-[0_0_0_1px_var(--destructive)] hover:shadow-[0_0_0_1px_var(--destructive)] [&_h3]:text-destructive [&>div]:bg-transparent [&>div]:pt-0 [&>div]:shadow-none" title="Danger zone" description="Delete this workspace and remove access for all members.">
      <Button type="button" variant="destructive" onClick={() => { setConfirmation(''); deletion.reset(); setOpen(true) }}>Delete workspace</Button>
    </SettingsCard>
    {open ? createPortal(
      <Modal title="Delete workspace?" description={`Deleting "${workspace.name}" removes access to its projects and tasks for everyone.`} onClose={close} maxWidth={480}>
        <form onSubmit={(event) => { event.preventDefault(); void deleteConfirmed() }}>
          {workspaces.length === 1 ? <p className="my-3 text-[13px] text-destructive">This is your last workspace. After deletion, you will have no workspace access.</p> : null}
          <div className="w-full min-w-0">
            <Label className="mb-1.5 h-4 gap-1 text-[13px] leading-4 font-medium text-muted-foreground" htmlFor={inputId}>Confirm workspace name</Label>
            <p className="text-xs">Type <strong>{workspace.name}</strong> to confirm deletion.</p>
            <Input id={inputId} autoComplete="off" required value={confirmation} readOnly={deletion.isPending} onChange={(event) => setConfirmation(event.target.value)} />
          </div>
          {deletion.isError ? <p role="alert" className="my-3 text-[13px] text-destructive">Workspace deletion failed. Try again, or refresh the page if the workspace has changed.</p> : null}
          <div className="mt-4 flex flex-wrap items-center justify-end gap-2 border-t border-border pt-4">
            <Button type="button" variant="ghost" disabled={deletion.isPending} onClick={close}>Cancel</Button>
            <Button type="submit" variant="destructive" className="aria-disabled:cursor-not-allowed aria-disabled:opacity-50" disabled={confirmation !== workspace.name} aria-disabled={deletion.isPending || undefined}>{deletion.isPending ? 'Deleting…' : 'Delete workspace'}</Button>
          </div>
        </form>
      </Modal>, document.body,
    ) : null}
  </>
}
