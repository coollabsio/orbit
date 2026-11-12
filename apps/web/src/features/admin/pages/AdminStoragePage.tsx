import { toast } from 'sonner'
import type { BackupSchedule, CompressionStatus, EnvironmentView, S3View, StorageView } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { LoadingText, Spinner } from '@/components/ui/spinner'
import { Switch } from '@/components/ui/switch'
import { useInstanceSettings, useRemoveS3, useSaveS3, useSaveStorageOptions, useStartImageCompression } from '@/features/admin/api'
import { SetByEnvironment } from '@/features/admin/components/SetByEnvironment'
import { S3BucketForm } from '@/features/settings/components/S3BucketForm'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { formatSize } from '@/lib/attachmentLib'

// `items` lets Select.Value render the option label instead of the raw value.
const SCHEDULE_OPTIONS: { value: BackupSchedule; label: string }[] = [
  { value: 'off', label: 'Off' },
  { value: 'hourly', label: 'Every hour' },
  { value: 'daily', label: 'Every day' },
]

const KEEP_DELETED_DAYS = [1, 7, 14, 30, 40, 90, 180, 365]

function daysLabel(days: number) {
  return days === 1 ? '1 day' : `${days} days`
}

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** Where attachments and backups are stored: this server's disk or an S3 bucket, and automatic backups. */
export function AdminStoragePage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="Storage"><p><LoadingText>Loading settings…</LoadingText></p></SettingsCard>
  if (settings.isError) return <SettingsCard title="Storage"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  const { storage, environment } = settings.data
  return (
    <>
      {/* Remount on save so the form shows what the server stored. */}
      <BucketCard key={JSON.stringify(storage.s3)} s3={storage.s3 ?? null} locked={environment.s3} />
      <UsageCard storage={storage} environment={environment} />
      {storage.image_compression ? <ImagesCard status={storage.image_compression} /> : null}
    </>
  )
}

function BucketCard({ s3, locked }: { s3: S3View | null; locked: boolean }) {
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
      disabled={locked}
      onSave={(body) => save.mutate(body, {
        onSuccess: () => toast('Bucket saved'),
        onError: (error) => toast.error(problemText(error, 'The bucket could not be saved.')),
      })}
      onRemove={s3 ? () => void removeBucket() : undefined}
    >
      {locked ? <SetByEnvironment variable="ORBIT_S3_*" className="mt-4" /> : null}
    </S3BucketForm>
  )
}

function UsageCard({ storage, environment }: { storage: StorageView; environment: EnvironmentView }) {
  const save = useSaveStorageOptions()
  const ready = Boolean(storage.s3)
  function change(options: Partial<Pick<StorageView, 'attachments_in_s3' | 'backups_in_s3' | 'backup_schedule' | 's3_keep_deleted_days'>>, message: string) {
    save.mutate({
      attachments_in_s3: storage.attachments_in_s3,
      backups_in_s3: storage.backups_in_s3,
      backup_schedule: storage.backup_schedule,
      s3_keep_deleted_days: storage.s3_keep_deleted_days,
      ...options,
    }, {
      onSuccess: () => toast(message),
      onError: (error) => toast.error(problemText(error, 'Storage settings could not be saved.')),
    })
  }
  const keepOptions = [...new Set([...KEEP_DELETED_DAYS, storage.s3_keep_deleted_days])]
    .sort((left, right) => left - right)
    .map((days) => ({ value: String(days), label: daysLabel(days) }))
  const moving = storage.files_to_move > 0
    ? `${storage.files_to_move} ${storage.files_to_move === 1 ? 'file' : 'files'} left to move ${storage.attachments_in_s3 ? 'to the bucket' : 'to this server'}.`
    : null
  return (
    <SettingsCard title="Usage" description="What Orbit keeps in the bucket, and how often it backs up by itself." flush>
      <div className="flex flex-col divide-y">
        <SettingsRow className="flex-nowrap">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
            {ready ? <span className="inline-flex items-center gap-1.5 font-medium text-foreground">Files in S3 <InfoTip text="All uploaded files: task and comment attachments, doc and chat files, custom emoji, stickers, mail attachments and profile pictures. On: new files go to the bucket and existing files move there in the background. Off: they move back to this server." /></span> : <span className="font-medium text-foreground">Files in S3</span>}
            {ready ? null : <span className="text-xs text-muted-foreground">Save a bucket above first.</span>}
            {moving ? (
              <span role="status" className="flex items-center gap-1.5 text-xs text-muted-foreground">
                {/* The spinner stops when moving failed; Orbit only tries again later. */}
                {storage.move_error ? null : <Spinner role="presentation" aria-hidden aria-label={undefined} className="size-3.5" />}
                {moving}
              </span>
            ) : null}
            {storage.move_error ? <span role="alert" className="text-xs text-destructive">Moving files stopped: {storage.move_error} Orbit tries again every ten minutes.</span> : null}
            {environment.attachments_in_s3 ? <SetByEnvironment variable="ORBIT_S3_ATTACHMENTS" /> : null}
          </div>
          <Switch
            aria-label="Files in S3"
            checked={storage.attachments_in_s3}
            disabled={environment.attachments_in_s3 || (!ready && !storage.attachments_in_s3) || save.isPending}
            onCheckedChange={(checked: boolean) => change({ attachments_in_s3: checked }, checked ? 'Files go to S3' : 'Files move back to this server')}
          />
        </SettingsRow>
        <SettingsRow className="flex-nowrap">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
            {ready ? <span className="inline-flex items-center gap-1.5 font-medium text-foreground">Backups in S3 <InfoTip text="On: each new backup is uploaded to the bucket and removed from this server, so a lost server loses no backups." /></span> : <span className="font-medium text-foreground">Backups in S3</span>}
            {ready ? null : <span className="text-xs text-muted-foreground">Save a bucket above first.</span>}
            {environment.backups_in_s3 ? <SetByEnvironment variable="ORBIT_S3_BACKUPS" /> : null}
          </div>
          <Switch
            aria-label="Backups in S3"
            checked={storage.backups_in_s3}
            disabled={environment.backups_in_s3 || (!ready && !storage.backups_in_s3) || save.isPending}
            onCheckedChange={(checked: boolean) => change({ backups_in_s3: checked }, checked ? 'Backups go to S3' : 'Backups stay on this server')}
          />
        </SettingsRow>
        <SettingsRow className="flex-nowrap">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
            <span className="inline-flex items-center gap-1.5 font-medium text-foreground">Automatic backups <InfoTip text="Orbit keeps the newest backup of each of the last 7 days and 4 weeks." /></span>
            {environment.backup_schedule ? <SetByEnvironment variable="ORBIT_AUTOMATIC_BACKUPS" /> : null}
          </div>
          <Select
            items={SCHEDULE_OPTIONS}
            value={storage.backup_schedule}
            disabled={environment.backup_schedule || save.isPending}
            onValueChange={(value) => change({ backup_schedule: value as BackupSchedule }, 'Backup schedule saved')}
          >
            <SelectTrigger aria-label="Automatic backups" className="w-36"><SelectValue /></SelectTrigger>
            <SelectContent>
              {SCHEDULE_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </SettingsRow>
        {ready ? (
          <SettingsRow className="flex-nowrap">
            <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
              <span className="inline-flex items-center gap-1.5 font-medium text-foreground">Keep deleted files in S3 <InfoTip text="A deleted or replaced attachment file (for example an image original after Compress images) stays in the bucket this long, then Orbit deletes it. Backups do not contain S3 files: restoring a backup older than this can miss files. The default, 40 days, covers all kept backups." /></span>
              {environment.s3_keep_deleted_days ? <SetByEnvironment variable="ORBIT_S3_KEEP_DELETED_DAYS" /> : null}
            </div>
            <Select
              items={keepOptions}
              value={String(storage.s3_keep_deleted_days)}
              disabled={environment.s3_keep_deleted_days || save.isPending}
              onValueChange={(value) => change({ s3_keep_deleted_days: Number(value) }, 'Deleted files setting saved')}
            >
              <SelectTrigger aria-label="Keep deleted files in S3" className="w-36"><SelectValue /></SelectTrigger>
              <SelectContent>
                {keepOptions.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </SettingsRow>
        ) : null}
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
        <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[0.8125rem]">
          <span className="inline-flex items-center gap-1.5 font-medium text-foreground">Compress images <InfoTip text="Scales JPEG, PNG and WebP images of 512 KB or more to at most 2560 px and saves them as WebP. The originals are replaced; GIFs and other files stay as they are." /></span>
          {status.running ? <LoadingText className="text-xs">Compressing… {done} {done === 1 ? 'file' : 'files'} checked. {summary}</LoadingText> : null}
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
