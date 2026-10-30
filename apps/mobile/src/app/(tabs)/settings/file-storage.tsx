import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { FieldLabel, Note, OptionHint, SettingsCard, SettingsShell, Toggle, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { bucketMovingText, type WorkspaceBucketView } from '../../../data/account-api'
import { s3Body, s3Draft } from '../../../data/admin-api'
import { useWorkspaceBucket, useWorkspaceBucketWrites } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { errorMessage } from '../../../session-context'
import { Button, Input, Spinner } from '../../../ui'

const TITLE = 'File storage'
const DESCRIPTION =
  "Keep this workspace's files in your own S3-compatible bucket (AWS S3, Cloudflare R2, …) instead of Orbit's storage. After you save, new files go to the bucket and existing files move there in the background. Orbit writes, reads and deletes a test file before it saves. The endpoint must be an https URL on the public internet."

/** The web's File storage page: the workspace's own S3 bucket for attachments. */
export default function FileStorageScreen() {
  const { workspace } = useOrbit()
  const bucket = useWorkspaceBucket()
  const allowed = workspace.permissions.includes('storage.manage')
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} refreshing={bucket.isRefetching} onRefresh={allowed ? () => void bucket.refetch() : undefined}>
      {!allowed ? (
        <SettingsCard title={TITLE}>
          <Note>Only the owner and admins of this workspace manage its file storage.</Note>
        </SettingsCard>
      ) : bucket.data ? (
        <BucketCard key={workspace.id} view={bucket.data} />
      ) : (
        <SettingsCard title={TITLE} description={DESCRIPTION}>
          {bucket.isError ? (
            <View className="flex-row items-center gap-2">
              <Note error>File storage could not be loaded.</Note>
              <Button label="Retry" variant="ghost" onPress={() => void bucket.refetch()} />
            </View>
          ) : (
            <Note>Loading file storage…</Note>
          )}
        </SettingsCard>
      )}
    </SettingsShell>
  )
}

function BucketCard({ view }: { view: WorkspaceBucketView }) {
  const { save, remove } = useWorkspaceBucketWrites()
  const s3 = view.s3 ?? null
  const [draft, setDraft] = useState(() => s3Draft(s3))
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const edit = (change: Partial<typeof draft>) => {
    save.reset()
    setDraft((current) => ({ ...current, ...change }))
  }
  const set = (field: 'endpoint' | 'region' | 'bucket' | 'prefix' | 'access_key_id' | 'secret_access_key') => (value: string) => edit({ [field]: value })
  // after a save or a removal the form shows what the server stored
  const stored = { onSuccess: (next: WorkspaceBucketView) => setDraft(s3Draft(next.s3 ?? null)) }
  const ready = draft.endpoint.trim() !== '' && draft.bucket.trim() !== '' && draft.access_key_id.trim() !== '' && (s3 !== null || draft.secret_access_key !== '')
  const moving = bucketMovingText(view)
  const plain = { autoCapitalize: 'none', autoCorrect: false } as const

  return (
    <SettingsCard title={TITLE} description={DESCRIPTION}>
      <View className="gap-4">
        {view.removing ? <Note>Removing: files move back to Orbit's storage.</Note> : null}
        {moving ? (
          <View accessibilityRole="text" className="flex-row items-center gap-1.5">
            {/* the spinner stops when moving failed; Orbit only tries again later */}
            {view.move_error ? null : <Spinner />}
            <OptionHint>{moving}</OptionHint>
          </View>
        ) : null}
        {view.move_error ? <OptionHint error>Moving files stopped: {view.move_error} Orbit tries again every ten minutes.</OptionHint> : null}
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
          {s3 && !view.removing ? (
            <Button
              label="Remove"
              variant="destructive"
              disabled={remove.isPending}
              onPress={() =>
                setConfirm({
                  title: 'Remove the bucket?',
                  description: "New files go to Orbit's storage. Files in the bucket move back and are deleted from it.",
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
