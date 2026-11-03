import { toast } from 'sonner'
import type { BackupSchedule, CompressionStatus, S3View, StorageView } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { Switch } from '@/components/ui/switch'
import { useInstanceSettings, useRemoveS3, useSaveS3, useSaveStorageOptions, useStartImageCompression } from '@/features/admin/api'
import { S3BucketForm } from '@/features/settings/components/S3BucketForm'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { formatSize } from '@/lib/attachmentLib'

// `items` lets Select.Value render the option label instead of the raw value.
const SCHEDULE_OPTIONS: { value: BackupSchedule; label: string }[] = [
  { value: 'off', label: 'Off' },
  { value: 'hourly', label: 'Every hour' },
  { value: 'daily', label: 'Every day' },
]

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** Where attachments and backups are stored: this server's disk or an S3 bucket, and automatic backups. */
export function AdminStoragePage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="Storage"><p role="status">Loading settings…</p></SettingsCard>
  if (settings.isError) return <SettingsCard title="Storage"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  const storage = settings.data.storage
  return (
    <>
      {/* Remount on save so the form shows what the server stored. */}
      <BucketCard key={JSON.stringify(storage.s3)} s3={storage.s3 ?? null} />
      <UsageCard storage={storage} />
      {storage.image_compression ? <ImagesCard status={storage.image_compression} /> : null}
    </>
  )
}

function BucketCard({ s3 }: { s3: S3View | null }) {
  const save = useSaveS3()
  const remove = useRemoveS3()

  async function removeBucket() {
    const confirmed = await confirmAction({
      title: 'Remove the bucket?',
      description: 'Orbit stops using it. Backups already in the bucket stay there.',
      confirmLabel: 'Remove',
      danger: true,
    })
    if (!confirmed) return
    remove.mutate(undefined, {
      onSuccess: () => toast('Bucket removed'),
      onError: (error) => toast.error(problemText(error, 'The bucket could not be removed.')),
    })
  }

  return (
    <S3BucketForm
      s3={s3}
      title="S3 bucket"
      description="An S3-compatible bucket (AWS S3, Cloudflare R2, MinIO, …) for attachments and backups. Orbit writes, reads and deletes a test file before it saves."
      endpointPlaceholder="https://s3.eu-central-1.amazonaws.com"
      saving={save.isPending}
      removing={remove.isPending}
      onSave={(body) => save.mutate(body, {
        onSuccess: () => toast('Bucket saved'),
        onError: (error) => toast.error(problemText(error, 'The bucket could not be saved.')),
      })}
      onRemove={s3 ? () => void removeBucket() : undefined}
    />
  )
}

function UsageCard({ storage }: { storage: StorageView }) {
  const save = useSaveStorageOptions()
  const ready = Boolean(storage.s3)
  function change(options: Partial<Pick<StorageView, 'attachments_in_s3' | 'backups_in_s3' | 'backup_schedule'>>, message: string) {
    save.mutate({
      attachments_in_s3: storage.attachments_in_s3,
      backups_in_s3: storage.backups_in_s3,
      backup_schedule: storage.backup_schedule,
      ...options,
    }, {
      onSuccess: () => toast(message),
      onError: (error) => toast.error(problemText(error, 'Storage settings could not be saved.')),
    })
  }
  const moving = storage.files_to_move > 0
    ? `${storage.files_to_move} ${storage.files_to_move === 1 ? 'file' : 'files'} left to move ${storage.attachments_in_s3 ? 'to the bucket' : 'to this server'}.`
    : null
  return (
    <SettingsCard title="Usage" description="What Orbit keeps in the bucket, and how often it backs up by itself." flush>
      <div className="flex flex-col divide-y">
        <SettingsRow className="flex-nowrap">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
            <span className="font-medium text-foreground">Attachments in S3</span>
            <span className="text-xs text-muted-foreground">
              {ready
                ? 'On: new files go to the bucket and existing files move there in the background. Off: they move back to this server.'
                : 'Save a bucket above first.'}
            </span>
            {moving ? (
              <span role="status" className="flex items-center gap-1.5 text-xs text-muted-foreground">
                {/* The spinner stops when moving failed; Orbit only tries again later. */}
                {storage.move_error ? null : <Spinner role="presentation" aria-hidden aria-label={undefined} className="size-3.5" />}
                {moving}
              </span>
            ) : null}
            {storage.move_error ? <span role="alert" className="text-xs text-destructive">Moving files stopped: {storage.move_error} Orbit tries again every ten minutes.</span> : null}
          </div>
          <Switch
            aria-label="Attachments in S3"
            checked={storage.attachments_in_s3}
            disabled={(!ready && !storage.attachments_in_s3) || save.isPending}
            onCheckedChange={(checked: boolean) => change({ attachments_in_s3: checked }, checked ? 'Attachments go to S3' : 'Attachments move back to this server')}
          />
        </SettingsRow>
        <SettingsRow className="flex-nowrap">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
            <span className="font-medium text-foreground">Backups in S3</span>
            <span className="text-xs text-muted-foreground">
              {ready
                ? 'On: each new backup is uploaded to the bucket and removed from this server, so a lost server loses no backups.'
                : 'Save a bucket above first.'}
            </span>
          </div>
          <Switch
            aria-label="Backups in S3"
            checked={storage.backups_in_s3}
            disabled={(!ready && !storage.backups_in_s3) || save.isPending}
            onCheckedChange={(checked: boolean) => change({ backups_in_s3: checked }, checked ? 'Backups go to S3' : 'Backups stay on this server')}
          />
        </SettingsRow>
        <SettingsRow className="flex-nowrap">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
            <span className="font-medium text-foreground">Automatic backups</span>
            <span className="text-xs text-muted-foreground">Orbit keeps the newest backup of each of the last 7 days and 4 weeks.</span>
          </div>
          <Select
            items={SCHEDULE_OPTIONS}
            value={storage.backup_schedule}
            disabled={save.isPending}
            onValueChange={(value) => change({ backup_schedule: value as BackupSchedule }, 'Backup schedule saved')}
          >
            <SelectTrigger aria-label="Automatic backups" className="w-36"><SelectValue /></SelectTrigger>
            <SelectContent>
              {SCHEDULE_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </SettingsRow>
      </div>
    </SettingsCard>
  )
}

/** Makes images that were uploaded before the client shrank them smaller, on this server and in S3. */
function ImagesCard({ status }: { status: CompressionStatus }) {
  const start = useStartImageCompression()
  const done = status.compressed + status.skipped
  const summary = status.compressed > 0
    ? `${status.compressed} ${status.compressed === 1 ? 'image' : 'images'} compressed: ${formatSize(status.bytes_before)} → ${formatSize(status.bytes_after)}.`
    : 'No image was compressed.'
  return (
    <SettingsCard title="Images" description="Large images keep their original size if they were uploaded before Orbit made images smaller on upload." flush>
      <SettingsRow className="flex-nowrap">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
          <span className="font-medium text-foreground">Compress images</span>
          <span className="text-xs text-muted-foreground">Scales JPEG, PNG and WebP images of 512 KB or more to at most 2560 px and saves them as WebP. The originals are replaced; GIFs and other files stay as they are.</span>
          {status.running ? <span role="status" className="text-xs text-muted-foreground">Compressing… {done} {done === 1 ? 'file' : 'files'} checked. {summary}</span> : null}
          {!status.running && done > 0 ? <span role="status" className="text-xs text-muted-foreground">Done. {summary}</span> : null}
          {status.error ? <span role="alert" className="text-xs text-destructive">Compression stopped: {status.error}</span> : null}
        </div>
        <Button
          type="button"
          variant="outline"
          disabled={status.running || start.isPending}
          onClick={() => start.mutate(undefined, { onError: (error) => toast.error(problemText(error, 'Compression could not start.')) })}
        >
          {status.running ? 'Compressing…' : 'Compress images'}
        </Button>
      </SettingsRow>
    </SettingsCard>
  )
}
