import { useState } from 'react'
import { toast } from 'sonner'
import type { BackupSchedule, CompressionStatus, S3View, StorageView } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { Switch } from '@/components/ui/switch'
import { useInstanceSettings, useRemoveS3, useSaveS3, useSaveStorageOptions, useStartImageCompression } from '@/features/admin/api'
import { FieldGrid, RequiredMark, SettingsRow } from '@/features/settings/components/SettingsParts'
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
  const [draft, setDraft] = useState({
    endpoint: s3?.endpoint ?? '',
    region: s3?.region ?? '',
    bucket: s3?.bucket ?? '',
    prefix: s3?.prefix ?? '',
    access_key_id: s3?.access_key_id ?? '',
    secret_access_key: '',
    path_style: s3?.path_style ?? false,
  })
  const set = (field: keyof typeof draft) => (event: React.ChangeEvent<HTMLInputElement>) => setDraft((current) => ({ ...current, [field]: event.target.value }))

  function submit(event: React.FormEvent) {
    event.preventDefault()
    save.mutate({
      endpoint: draft.endpoint.trim(),
      region: draft.region.trim() || null,
      bucket: draft.bucket.trim(),
      prefix: draft.prefix.trim() || null,
      access_key_id: draft.access_key_id.trim(),
      // An empty field keeps the saved secret key.
      secret_access_key: draft.secret_access_key || undefined,
      path_style: draft.path_style,
    }, {
      onSuccess: () => toast('Bucket saved'),
      onError: (error) => toast.error(problemText(error, 'The bucket could not be saved.')),
    })
  }

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
    <form onSubmit={submit}>
      <SettingsCard
        title="S3 bucket"
        description="An S3-compatible bucket (AWS S3, Cloudflare R2, MinIO, …) for attachments and backups. Orbit writes, reads and deletes a test file before it saves."
      >
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="s3-endpoint">Endpoint <RequiredMark /></FieldLabel>
            <Input id="s3-endpoint" required type="url" placeholder="https://s3.eu-central-1.amazonaws.com" value={draft.endpoint} onChange={set('endpoint')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="s3-region">Region</FieldLabel>
            <Input id="s3-region" placeholder="us-east-1" value={draft.region} onChange={set('region')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="s3-bucket">Bucket <RequiredMark /></FieldLabel>
            <Input id="s3-bucket" required placeholder="orbit-files" value={draft.bucket} onChange={set('bucket')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="s3-prefix">Prefix</FieldLabel>
            <Input id="s3-prefix" placeholder="orbit" value={draft.prefix} onChange={set('prefix')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="s3-access-key">Access key ID <RequiredMark /></FieldLabel>
            <Input id="s3-access-key" required autoComplete="off" value={draft.access_key_id} onChange={set('access_key_id')} />
          </Field>
          <Field>
            <FieldLabel htmlFor="s3-secret-key">Secret access key {s3 ? null : <RequiredMark />}</FieldLabel>
            <PasswordInput id="s3-secret-key" autoComplete="new-password" required={!s3} placeholder={s3 ? 'Saved. Leave empty to keep it.' : ''} value={draft.secret_access_key} onChange={set('secret_access_key')} />
          </Field>
        </FieldGrid>
        <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
          <label className="flex items-center gap-2 text-[13px]">
            <Switch checked={draft.path_style} onCheckedChange={(checked: boolean) => setDraft((current) => ({ ...current, path_style: checked }))} />
            Path-style URLs (MinIO and most self-hosted servers)
          </label>
          <div className="flex items-center gap-2">
            {s3 ? <Button type="button" variant="ghost" className="text-destructive" disabled={remove.isPending} onClick={() => void removeBucket()}>Remove</Button> : null}
            <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Testing bucket…' : 'Save bucket'}</Button>
          </div>
        </div>
      </SettingsCard>
    </form>
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
