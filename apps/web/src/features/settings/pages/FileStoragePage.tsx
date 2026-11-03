import { toast } from 'sonner'
import type { WorkspaceBucketView } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { S3BucketForm } from '@/features/settings/components/S3BucketForm'
import { useRemoveWorkspaceBucket, useSaveWorkspaceBucket, useWorkspaceBucket } from '@/features/workspaces/api'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

const TITLE = 'File storage'

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** The workspace's own S3 bucket for its files, instead of the installation's storage. */
export function FileStoragePage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('storage.manage')
  const bucket = useWorkspaceBucket(workspace.id, canManage)
  if (!canManage) return <SettingsCard title={TITLE}><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage file storage.</p></SettingsCard>
  if (bucket.isPending) return <SettingsCard title={TITLE}><p role="status">Loading settings…</p></SettingsCard>
  if (bucket.isError) return <SettingsCard title={TITLE}><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void bucket.refetch()}>Retry</Button></p></SettingsCard>
  // Remount on save so the form shows what the server stored.
  return <BucketForm key={JSON.stringify(bucket.data.s3)} workspaceId={workspace.id} bucket={bucket.data} />
}

function BucketForm({ workspaceId, bucket }: { workspaceId: string; bucket: WorkspaceBucketView }) {
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
      onSuccess: () => toast("Files move back to Orbit's storage"),
      onError: (error) => toast.error(problemText(error, 'The bucket could not be removed.')),
    })
  }

  const count = `${bucket.files_to_move} ${bucket.files_to_move === 1 ? 'file' : 'files'}`
  const moving = bucket.checking
    ? 'Checking which files to move…'
    : bucket.files_to_move > 0
      ? (bucket.removing ? `${count} left to move back to Orbit's storage.` : `${count} left to move to your bucket.`)
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
        onSuccess: () => toast('Bucket saved'),
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
        {bucket.move_error ? <span role="alert" className="text-destructive">Moving files stopped: {bucket.move_error} Orbit tries again every ten minutes.</span> : null}
      </div>
    </S3BucketForm>
  )
}
