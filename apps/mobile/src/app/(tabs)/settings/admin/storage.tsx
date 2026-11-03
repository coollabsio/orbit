import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { AdminShell, FieldLabel, Note, OptionHint, OptionText, Segmented, SettingsCard, SettingsRow, Toggle } from '../../../../components/settings/Shell'
import {
  BACKUP_SCHEDULE_OPTIONS,
  type CompressionStatus,
  compressionText,
  movingText,
  s3Body,
  s3Draft,
  type S3View,
  storageOptions,
  type StorageView,
} from '../../../../data/admin-api'
import { useInstanceSettings, useRemoveS3, useSaveS3, useSaveStorageOptions, useStartImageCompression } from '../../../../hooks/admin'
import { errorMessage } from '../../../../session-context'
import { Button, Input, Spinner } from '../../../../ui'

/** The web's Admin → Storage: an S3 bucket for attachments and backups, what goes there, and image compression. */
export default function AdminStorageScreen() {
  const settings = useInstanceSettings()
  const storage = settings.data?.storage
  return (
    <AdminShell refreshing={settings.isRefetching} onRefresh={() => void settings.refetch()}>
      {settings.isPending ? <SettingsCard title="Storage"><Note>Loading settings…</Note></SettingsCard> : null}
      {settings.isError ? (
        <SettingsCard title="Storage">
          <View className="flex-row items-center gap-2">
            <Note error>Settings could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void settings.refetch()} />
          </View>
        </SettingsCard>
      ) : null}
      {storage ? (
        <>
          <BucketCard s3={storage.s3 ?? null} />
          <UsageCard storage={storage} />
          {storage.image_compression ? <ImagesCard status={storage.image_compression} /> : null}
        </>
      ) : null}
    </AdminShell>
  )
}

function BucketCard({ s3 }: { s3: S3View | null }) {
  const save = useSaveS3()
  const remove = useRemoveS3()
  const [draft, setDraft] = useState(() => s3Draft(s3))
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const edit = (change: Partial<typeof draft>) => {
    save.reset()
    setDraft((current) => ({ ...current, ...change }))
  }
  const set = (field: 'endpoint' | 'region' | 'bucket' | 'prefix' | 'access_key_id' | 'secret_access_key') => (value: string) => edit({ [field]: value })
  // after a save or a removal the form shows what the server stored
  const stored = { onSuccess: (settings: { storage: StorageView }) => setDraft(s3Draft(settings.storage.s3 ?? null)) }
  const ready = draft.endpoint.trim() !== '' && draft.bucket.trim() !== '' && draft.access_key_id.trim() !== '' && (s3 !== null || draft.secret_access_key !== '')
  const plain = { autoCapitalize: 'none', autoCorrect: false } as const

  return (
    <SettingsCard
      title="S3 bucket"
      description="An S3-compatible bucket (AWS S3, Cloudflare R2, MinIO, …) for attachments and backups. Orbit writes, reads and deletes a test file before it saves."
    >
      <View className="gap-4">
        <Input label="Endpoint" required placeholder="https://s3.eu-central-1.amazonaws.com" keyboardType="url" {...plain} value={draft.endpoint} onChangeText={set('endpoint')} />
        <Input label="Region" placeholder="us-east-1" {...plain} value={draft.region} onChangeText={set('region')} />
        <Input label="Bucket" required placeholder="orbit-files" {...plain} value={draft.bucket} onChangeText={set('bucket')} />
        <Input label="Prefix" placeholder="orbit" {...plain} value={draft.prefix} onChangeText={set('prefix')} />
        <Input label="Access key ID" required autoComplete="off" {...plain} value={draft.access_key_id} onChangeText={set('access_key_id')} />
        <Input
          label="Secret access key"
          required={!s3}
          secureTextEntry
          autoComplete="new-password"
          {...plain}
          placeholder={s3 ? 'Saved. Leave empty to keep it.' : undefined}
          value={draft.secret_access_key}
          onChangeText={set('secret_access_key')}
        />
        <View className="flex-row items-center gap-3">
          <View className="flex-1 min-w-0">
            <FieldLabel>Path-style URLs</FieldLabel>
            <OptionHint>MinIO and some other S3-compatible services.</OptionHint>
          </View>
          <Toggle label="Path-style URLs" value={draft.path_style} onChange={(path_style) => edit({ path_style })} />
        </View>
        {save.isError ? <Note error>{errorMessage(save.error)}</Note> : null}
        {remove.isError ? <Note error>{errorMessage(remove.error)}</Note> : null}
        {save.isSuccess ? <Note>Bucket saved.</Note> : null}
        <View className="flex-row flex-wrap justify-end gap-2">
          {s3 ? (
            <Button
              label="Remove"
              variant="destructive"
              disabled={remove.isPending}
              onPress={() =>
                setConfirm({
                  title: 'Remove the bucket?',
                  description: 'Orbit stops using it. Backups already in the bucket stay there.',
                  confirmLabel: 'Remove',
                  danger: true,
                  onConfirm: () => remove.mutate(undefined, stored),
                })
              }
            />
          ) : null}
          <Button label={save.isPending ? 'Testing bucket…' : 'Save bucket'} disabled={save.isPending || !ready} onPress={() => save.mutate(s3Body(draft), stored)} />
        </View>
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}

function UsageCard({ storage }: { storage: StorageView }) {
  const save = useSaveStorageOptions()
  const ready = Boolean(storage.s3)
  const moving = movingText(storage)
  const change = (options: Parameters<typeof storageOptions>[1]) => save.mutate(storageOptions(storage, options))
  return (
    <SettingsCard title="Usage" description="What Orbit keeps in the bucket, and how often it backs up by itself." flush>
      <SettingsRow first>
        <OptionText title="Attachments in S3">
          <OptionHint>
            {ready ? 'On: new files go to the bucket and existing files move there in the background. Off: they move back to this server.' : 'Save a bucket above first.'}
          </OptionHint>
          {moving ? (
            <View accessibilityRole="text" className="flex-row items-center gap-1.5">
              {/* the spinner stops when moving failed; Orbit only tries again later */}
              {storage.move_error ? null : <Spinner />}
              <OptionHint>{moving}</OptionHint>
            </View>
          ) : null}
          {storage.move_error ? <OptionHint error>Moving files stopped: {storage.move_error} Orbit tries again every ten minutes.</OptionHint> : null}
        </OptionText>
        <Toggle
          label="Attachments in S3"
          value={storage.attachments_in_s3}
          disabled={(!ready && !storage.attachments_in_s3) || save.isPending}
          onChange={(attachments_in_s3) => change({ attachments_in_s3 })}
        />
      </SettingsRow>
      <SettingsRow>
        <OptionText title="Backups in S3">
          <OptionHint>{ready ? 'On: each new backup is uploaded to the bucket and removed from this server, so a lost server loses no backups.' : 'Save a bucket above first.'}</OptionHint>
        </OptionText>
        <Toggle label="Backups in S3" value={storage.backups_in_s3} disabled={(!ready && !storage.backups_in_s3) || save.isPending} onChange={(backups_in_s3) => change({ backups_in_s3 })} />
      </SettingsRow>
      <View className="gap-2 border-t border-border px-4 py-3">
        <OptionText title="Automatic backups">
          <OptionHint>Orbit keeps the newest backup of each of the last 7 days and 4 weeks.</OptionHint>
        </OptionText>
        <Segmented label="Automatic backups" value={storage.backup_schedule} options={BACKUP_SCHEDULE_OPTIONS} onChange={(backup_schedule) => (save.isPending ? undefined : change({ backup_schedule }))} />
      </View>
      {save.isError ? <View className="border-t border-border px-4 py-3"><Note error>{errorMessage(save.error)}</Note></View> : null}
    </SettingsCard>
  )
}

/** Makes images that were uploaded before the client shrank them smaller, on this server and in S3. */
function ImagesCard({ status }: { status: CompressionStatus }) {
  const start = useStartImageCompression()
  const progress = compressionText(status)
  return (
    <SettingsCard title="Images" description="Large images keep their original size if they were uploaded before Orbit made images smaller on upload." flush>
      <View className="gap-3 px-4 py-3">
        <OptionText title="Compress images">
          <OptionHint>Scales JPEG, PNG and WebP images of 512 KB or more to at most 2560 px and saves them as WebP. The originals are replaced; GIFs and other files stay as they are.</OptionHint>
          {progress ? <OptionHint>{progress}</OptionHint> : null}
          {status.error ? <OptionHint error>Compression stopped: {status.error}</OptionHint> : null}
          {start.isError ? <OptionHint error>{errorMessage(start.error)}</OptionHint> : null}
        </OptionText>
        <View className="flex-row justify-end">
          <Button label={status.running ? 'Compressing…' : 'Compress images'} variant="outline" disabled={status.running || start.isPending} onPress={() => start.mutate()} />
        </View>
      </View>
    </SettingsCard>
  )
}
