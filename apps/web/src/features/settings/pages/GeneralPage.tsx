import { useRef, useState } from 'react'
import type { StorageFile } from '@orbit/api/generated/types.gen'
import { canDeleteStorageFile, storageFileLabel, storageFilesPage } from '@orbit/api/storage'
import { toast } from 'sonner'
import { cn } from 'cn'
import { Link } from 'react-router'
import { confirmAction } from '@/components/common/confirmAction'
import { UnsavedBar } from '@/components/common/UnsavedBar'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Progress, ProgressLabel } from '@/components/ui/progress'
import { useDeleteStorageFile, useRenameWorkspace, useWorkspaceStorage } from '@/features/workspaces/api'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { SettingsCard } from '@/components/common/SettingsCard'
import { FieldGrid } from '@/features/settings/components/SettingsParts'
import { formatBytes } from '@/lib/formatBytes'
import { WorkspaceLeave } from '@/features/settings/components/WorkspaceLeave'
import { useSlowPending } from '@/lib/useDebouncedValue'

export function GeneralPage() {
  const { workspace } = useWorkspace()
  const renameWorkspace = useRenameWorkspace(workspace.id)
  const renameSlow = useSlowPending(renameWorkspace.isPending)
  const canRename = useCan('workspace.update')
  const formRef = useRef<HTMLFormElement>(null)
  const [nameDraft, setNameDraft] = useState<{ workspaceId: string; value: string } | null>(null)
  const name = nameDraft?.workspaceId === workspace.id ? nameDraft.value : workspace.name
  const dirty = name.trim() !== workspace.name

  function saveWorkspace() {
    if (!name.trim() || !dirty || renameWorkspace.isPending) return
    renameWorkspace.mutate(
      { name: name.trim(), version: workspace.version },
      { onSuccess: () => setNameDraft(null) },
    )
  }

  return (
    <>
      {canRename ? <SettingsCard title="Workspace" description="Rename this workspace.">
        <form ref={formRef} onSubmit={(event) => { event.preventDefault(); saveWorkspace() }}>
          <FieldGrid>
            <Field><FieldLabel htmlFor="workspace-name">Name</FieldLabel><Input id="workspace-name" required disabled={renameSlow} value={name} onChange={(event) => setNameDraft({ workspaceId: workspace.id, value: event.target.value })} /></Field>
          </FieldGrid>
        </form>
        {renameWorkspace.isError ? <p role="alert" className="text-destructive">Workspace rename failed. <Button variant="ghost" onClick={saveWorkspace}>Retry</Button></p> : null}
        {renameSlow ? <p role="status">Saving workspace…</p> : null}
      </SettingsCard> : null}

      <StorageCard workspaceId={workspace.id} />

      <WorkspaceLeave key={workspace.id} />
      {dirty ? (
        <UnsavedBar
          onReset={() => { if (renameWorkspace.isPending) return; setNameDraft(null); renameWorkspace.reset() }}
          onSave={() => formRef.current?.requestSubmit()}
          saving={renameWorkspace.isPending}
        />
      ) : null}
    </>
  )
}

/** How much of its storage quota the workspace uses. Every member sees it; admins also see (and can delete) the
    largest files. */
function StorageCard({ workspaceId }: { workspaceId: string }) {
  const [offset, setOffset] = useState(0)
  const storage = useWorkspaceStorage(workspaceId, offset)
  const files = storage.data?.largest_files ?? []
  const page = storageFilesPage(offset, files.length, storage.data?.file_count ?? 0)
  // a delete emptied the last page: go back one
  if (storage.data && !storage.isPlaceholderData && files.length === 0 && page.previous !== null) setOffset(page.previous)
  const deleteFile = useDeleteStorageFile(workspaceId)
  const used = storage.data?.used_bytes ?? 0
  const quota = storage.data?.quota_bytes ?? null
  const percent = quota ? Math.min(100, (used / quota) * 100) : 0
  const canManageStorage = useCan('storage.manage')

  async function remove(file: StorageFile) {
    const { name } = storageFileLabel(file)
    if (!await confirmAction({ title: `Delete ${name}?`, description: 'It frees its storage at once. Tasks, pages and messages that use it show “Deleted file” instead. You cannot undo this.', confirmLabel: 'Delete', danger: true })) return
    deleteFile.mutate(file.id, { onError: () => toast.error('The file could not be deleted.') })
  }

  return (
    <SettingsCard
      title="Storage"
      description="Files, emoji and stickers of this workspace. A file attached more than once counts once; a deleted file stops counting about a day after the trash is emptied."
      actions={canManageStorage ? <Button variant="outline" render={<Link to="/settings/file-storage" />} nativeButton={false}>Use your own bucket</Button> : null}
    >
      {storage.isPending ? <p role="status" className="text-[13px] text-muted-foreground">Loading storage…</p> : null}
      {storage.isError ? <p role="alert" className="text-[13px] text-destructive">Storage use could not be loaded.</p> : null}
      {storage.data ? (
        <Progress
          value={quota ? percent : null}
          className={cn('gap-2', !quota && '[&_[data-slot=progress-track]]:hidden', percent >= 90 && '[&_[data-slot=progress-indicator]]:bg-destructive')}
        >
          <ProgressLabel className="text-[13px] text-foreground">Used</ProgressLabel>
          <span className="ml-auto text-[13px] text-muted-foreground tabular-nums">
            {quota ? `${formatBytes(used)} of ${formatBytes(quota)}` : `${formatBytes(used)} · no limit`}
          </span>
        </Progress>
      ) : null}
      {storage.data?.largest_files.length ? (
        <div className="mt-5 flex flex-col gap-2">
          <h3 className="text-[13px] font-medium text-foreground">Largest files</h3>
          <ul className="flex flex-col divide-y rounded-lg border">
            {storage.data.largest_files.map((file) => {
              const { name, where } = storageFileLabel(file)
              return (
                <li key={file.id} className="flex items-center gap-3 px-3 py-2">
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-[13px] text-foreground">{name}</span>
                    <span className="truncate text-xs text-muted-foreground">{where}</span>
                  </div>
                  <span className="shrink-0 text-[13px] text-muted-foreground tabular-nums">{formatBytes(file.byte_size)}</span>
                  {canDeleteStorageFile(file) ? (
                    <Button variant="ghost" size="sm" aria-label={`Delete ${name}`} disabled={deleteFile.isPending} onClick={() => void remove(file)}>Delete</Button>
                  ) : null}
                </li>
              )
            })}
          </ul>
          {page.previous !== null || page.next !== null ? (
            <div className="flex items-center gap-2">
              <span className="mr-auto text-xs text-muted-foreground tabular-nums">{page.label}</span>
              <Button variant="outline" size="sm" disabled={page.previous === null || storage.isFetching} onClick={() => page.previous !== null && setOffset(page.previous)}>Previous</Button>
              <Button variant="outline" size="sm" disabled={page.next === null || storage.isFetching} onClick={() => page.next !== null && setOffset(page.next)}>Next</Button>
            </div>
          ) : null}
        </div>
      ) : null}
    </SettingsCard>
  )
}
