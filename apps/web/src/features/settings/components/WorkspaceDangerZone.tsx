import { useId, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { Modal } from '@/components/common/Modal'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { DialogFooter, DialogClose } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { useDeleteWorkspace } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { SettingsCard } from '@/components/common/SettingsCard'

export function WorkspaceDangerZone() {
  const { workspace, workspaces } = useWorkspace()
  const navigate = useNavigate()
  const deletion = useDeleteWorkspace(workspace.id)
  const [open, setOpen] = useState(false)
  const [confirmation, setConfirmation] = useState('')
  const inputId = useId()
  // Blocks a second submit before `deletion.isPending` has rendered.
  const deleting = useRef(false)

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
    {open ? (
      <Modal title="Delete workspace?" description={`Deleting "${workspace.name}" removes access to its projects and tasks for everyone.`} onClose={() => setOpen(false)} dismissible={!deletion.isPending} className="sm:max-w-120">
        <form onSubmit={(event) => { event.preventDefault(); void deleteConfirmed() }}>
          {workspaces.length === 1 ? <p className="my-3 text-[13px] text-destructive">This is your last workspace. After deletion, you will have no workspace access.</p> : null}
          <Field>
            <FieldLabel htmlFor={inputId}>Confirm workspace name</FieldLabel>
            <FieldDescription>Type <strong>{workspace.name}</strong> to confirm deletion.</FieldDescription>
            <Input id={inputId} autoComplete="off" required value={confirmation} readOnly={deletion.isPending} onChange={(event) => setConfirmation(event.target.value)} />
          </Field>
          {deletion.isError ? <p role="alert" className="my-3 text-[13px] text-destructive">Workspace deletion failed. Try again, or refresh the page if the workspace has changed.</p> : null}
          <DialogFooter className="mt-4">
            <DialogClose render={<Button type="button" variant="ghost" disabled={deletion.isPending} />}>Cancel</DialogClose>
            <Button type="submit" variant="destructive" className="aria-disabled:cursor-not-allowed aria-disabled:opacity-50" disabled={confirmation !== workspace.name} aria-disabled={deletion.isPending || undefined}>{deletion.isPending ? 'Deleting…' : 'Delete workspace'}</Button>
          </DialogFooter>
        </form>
      </Modal>
    ) : null}
  </>
}
