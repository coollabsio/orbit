import { useState } from 'react'
import { toast } from 'sonner'
import { cn } from 'cn'
import type { StorageFile, WorkspaceBucketView } from '@orbit/api/generated/types.gen'
import { canDeleteStorageFile, storageFileLabel, storageFilesPage } from '@orbit/api/storage'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Progress, ProgressLabel } from '@/components/ui/progress'
import { LoadingText, Spinner } from '@/components/ui/spinner'
import { S3BucketForm } from '@/features/settings/components/S3BucketForm'
import { useDeleteStorageFile, useRemoveWorkspaceBucket, useSaveWorkspaceBucket, useWorkspaceBucket, useWorkspaceStorage } from '@/features/workspaces/api'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { formatBytes } from '@/lib/formatBytes'

const TITLE = 'Own bucket'

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** The workspace's own S3 bucket, which only admins manage, and its storage use, which every member sees. */
export function FileStoragePage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('storage.manage')
  return (
    <>
      {canManage ? <BucketSettings workspaceId={workspace.id} /> : null}
      <StorageCard workspaceId={workspace.id} />
    </>
  )
}

/** The workspace's own S3 bucket for its files, instead of the installation's storage. */
function BucketSettings({ workspaceId }: { workspaceId: string }) {
  const bucket = useWorkspaceBucket(workspaceId, true)
  // The move that the admin started here, kept outside the form because a save remounts it. The form says when the
  // move is done, even if it was too quick to show any progress.
  const [started, setStarted] = useState<'save' | 'remove' | null>(null)
  if (bucket.isPending) return <SettingsCard title={TITLE}><p><LoadingText>Loading settings…</LoadingText></p></SettingsCard>
  if (bucket.isError) return <SettingsCard title={TITLE}><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void bucket.refetch()}>Retry</Button></p></SettingsCard>
  // Remount on save so the form shows what the server stored.
  return <BucketForm key={JSON.stringify(bucket.data.s3)} workspaceId={workspaceId} bucket={bucket.data} started={started} onStart={setStarted} />
}

function BucketForm({ workspaceId, bucket, started, onStart }: {
  workspaceId: string
  bucket: WorkspaceBucketView
  started: 'save' | 'remove' | null
  onStart: (move: 'save' | 'remove') => void
}) {
  const save = useSaveWorkspaceBucket(workspaceId)
  const remove = useRemoveWorkspaceBucket(workspaceId)

  async function removeBucket() {
    const confirmed = await confirmAction({
      title: 'Remove the bucket?',
      description: "New files go to Orbit's storage. Files in the bucket move back and are deleted from it.",
      confirmLabel: 'Remove',
      danger: true,
    })
    if (!confirmed) return
    remove.mutate(undefined, {
      onSuccess: () => {
        toast("Files move back to Orbit's storage")
        onStart('remove')
      },
      onError: (error) => toast.error(problemText(error, 'The bucket could not be removed.')),
    })
  }

  const count = `${bucket.files_to_move} ${bucket.files_to_move === 1 ? 'file' : 'files'}`
  const moving = bucket.checking
    ? 'Checking which files to move…'
    : bucket.files_to_move > 0
      ? (bucket.removing ? `${count} left to move back to Orbit's storage.` : `${count} left to move to your bucket.`)
      : null
  const moved = started && !moving && !bucket.removing && !bucket.move_error
    ? (started === 'remove' ? "Done. All files are back in Orbit's storage." : 'Done. All files are in your bucket.')
    : null
  return (
    <S3BucketForm
      s3={bucket.s3 ?? null}
      title={TITLE}
      description="Keep this workspace's files in your own S3-compatible bucket (AWS S3, Cloudflare R2, …) instead of Orbit's storage. After you save, new files go to the bucket and existing files move there in the background. Orbit writes, reads and deletes a test file before it saves. The endpoint must be an https URL on the public internet."
      endpointPlaceholder="https://s3.eu-central-1.amazonaws.com"
      saving={save.isPending}
      removing={remove.isPending}
      onSave={(body) => save.mutate(body, {
        onSuccess: () => {
          toast('Bucket saved')
          onStart('save')
        },
        onError: (error) => toast.error(problemText(error, 'The bucket could not be saved.')),
      })}
      onRemove={bucket.s3 && !bucket.removing ? () => void removeBucket() : undefined}
    >
      <div className="mt-4 flex flex-col gap-1 text-xs text-muted-foreground empty:hidden">
        {bucket.removing ? <span>Removing: files move back to Orbit's storage. Save the bucket again to keep using it.</span> : null}
        {moving ? (
          <span role="status" className="flex items-center gap-1.5">
            {/* The spinner stops when moving failed; Orbit only tries again later. */}
            {bucket.move_error ? null : <Spinner role="presentation" aria-hidden aria-label={undefined} className="size-3.5" />}
            {moving}
          </span>
        ) : null}
        {moved ? <span role="status">{moved}</span> : null}
        {bucket.move_error ? <span role="alert" className="text-destructive">Moving files stopped: {bucket.move_error} Orbit tries again every ten minutes.</span> : null}
      </div>
    </S3BucketForm>
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

  async function remove(file: StorageFile) {
    const { name } = storageFileLabel(file)
    if (!await confirmAction({ title: `Delete ${name}?`, description: 'It frees its storage at once. Tasks, pages and messages that use it show “Deleted file” instead. You cannot undo this.', confirmLabel: 'Delete', danger: true })) return
    deleteFile.mutate(file.id, { onError: () => toast.error('The file could not be deleted.') })
  }

  return (
    <SettingsCard
      title="Usage"
      description="Files, emoji and stickers of this workspace. A file attached more than once counts once; a deleted file stops counting about a day after the trash is emptied."
    >
      {storage.isPending ? <p className="text-[0.8125rem] text-muted-foreground"><LoadingText>Loading storage…</LoadingText></p> : null}
      {storage.isError ? <p role="alert" className="text-[0.8125rem] text-destructive">Storage use could not be loaded.</p> : null}
      {storage.data ? (
        <Progress
          value={quota ? percent : null}
          className={cn('gap-2', !quota && '[&_[data-slot=progress-track]]:hidden', percent >= 90 && '[&_[data-slot=progress-indicator]]:bg-destructive')}
        >
          <ProgressLabel className="text-[0.8125rem] text-foreground">Used</ProgressLabel>
          <span className="ml-auto text-[0.8125rem] text-muted-foreground tabular-nums">
            {quota ? `${formatBytes(used)} of ${formatBytes(quota)}` : `${formatBytes(used)} · no limit`}
          </span>
        </Progress>
      ) : null}
      {storage.data?.largest_files.length ? (
        <div className="mt-5 flex flex-col gap-2">
          <h3 className="text-[0.8125rem] font-medium text-foreground">Largest files</h3>
          <ul className="flex flex-col divide-y rounded-lg border">
            {storage.data.largest_files.map((file) => {
              const { name, where } = storageFileLabel(file)
              return (
                <li key={file.id} className="flex items-center gap-3 px-3 py-2">
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-[0.8125rem] text-foreground">{name}</span>
                    <span className="truncate text-xs text-muted-foreground">{where}</span>
                  </div>
                  <span className="shrink-0 text-[0.8125rem] text-muted-foreground tabular-nums">{formatBytes(file.byte_size)}</span>
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
