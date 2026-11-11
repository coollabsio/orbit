import { canDeleteStorageFile, storageFileLabel, storageFilesPage } from '@orbit/api/storage'
import type { StorageFile } from '@orbit/api/generated/types.gen'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../components/settings/Dialog'
import { FieldLabel, Note, OptionHint, SettingsCard, SettingsShell, Toggle, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { bucketMovingText, formatBytes, storageUse, type WorkspaceBucketView } from '../../../data/account-api'
import { s3Body, s3Draft } from '../../../data/admin-api'
import { useDeleteStorageFile, useWorkspaceBucket, useWorkspaceBucketWrites, useWorkspaceStorage } from '../../../hooks/account'
import { useOrbit } from '../../../orbit-context'
import { errorMessage } from '../../../session-context'
import { Button, Input, Spinner, Text, LabelTip } from '../../../ui'

const TITLE = 'Own bucket'
const DESCRIPTION =
  "Keep this workspace's files in your own S3-compatible bucket (AWS S3, Cloudflare R2, …) instead of Orbit's storage. After you save, new files go to the bucket and existing files move there in the background. Orbit writes, reads and deletes a test file before it saves. The endpoint must be an https URL on the public internet."

/** The web's File storage page: the workspace's storage use, and its own S3 bucket for attachments. */
export default function FileStorageScreen() {
  const { workspace } = useOrbit()
  const [offset, setOffset] = useState(0)
  const storage = useWorkspaceStorage(offset)
  const files = storage.data?.largest_files ?? []
  const page = storageFilesPage(offset, files.length, storage.data?.file_count ?? 0)
  // a delete emptied the last page: go back one
  if (storage.data && !storage.isPlaceholderData && files.length === 0 && page.previous !== null) setOffset(page.previous)
  const bucket = useWorkspaceBucket()
  const allowed = workspace.permissions.includes('storage.manage')
  return (
    <SettingsShell
      title="Settings"
      nav={WORKSPACE_NAV}
      onRefresh={() => Promise.all([storage.refetch(), allowed ? bucket.refetch() : null])}
    >
      {!allowed ? null : bucket.data ? (
        <BucketCard key={workspace.id} view={bucket.data} />
      ) : (
        <SettingsCard title={TITLE} description={DESCRIPTION}>
          {bucket.isError ? (
            <View className="flex-row items-center gap-2">
              <Note error>File storage could not be loaded.</Note>
              <Button label="Retry" variant="ghost" onPress={() => void bucket.refetch()} />
            </View>
          ) : (
            <Note loading>Loading file storage…</Note>
          )}
        </SettingsCard>
      )}
      <SettingsCard
        title="Usage"
        description="Files, emoji and stickers of this workspace. A file attached more than once counts once; a deleted file stops counting about a day after the trash is emptied."
      >
        {storage.data ? <StorageBar {...storageUse(storage.data)} /> : <Note error={storage.isError} loading={!storage.isError}>{storage.isError ? 'Storage use could not be loaded.' : 'Loading storage…'}</Note>}
        {files.length ? (
          <LargestFiles files={files} page={page} loading={storage.isFetching} onPage={setOffset} />
        ) : null}
      </SettingsCard>
    </SettingsShell>
  )
}

function BucketCard({ view }: { view: WorkspaceBucketView }) {
  const { save, remove } = useWorkspaceBucketWrites()
  const s3 = view.s3 ?? null
  const [draft, setDraft] = useState(() => s3Draft(s3))
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  // like the web form, Save is always on; a press with an empty required field says so instead of saving
  const [tried, setTried] = useState(false)
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
        {/* the web's switch with its label after it */}
        <View className="flex-row items-center gap-2">
          <Toggle label="Path-style URLs" value={draft.path_style} onChange={(path_style) => edit({ path_style })} />
          <View className="flex-1">
            <LabelTip label="Path-style URLs" text="For MinIO and some other S3-compatible services." className="text-[13px] leading-[18px] text-foreground" />
          </View>
        </View>
        {save.isError ? <Note error>{errorMessage(save.error)}</Note> : null}
        {remove.isError ? <Note error>{errorMessage(remove.error)}</Note> : null}
        {save.isSuccess ? <Note>Bucket saved.</Note> : null}
        {tried && !ready ? <Note error>Fill in the required fields.</Note> : null}
        <View className="flex-row flex-wrap gap-2">
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
          <Button label={save.isPending ? 'Testing bucket…' : 'Save bucket'} disabled={save.isPending} onPress={() => (ready ? save.mutate(s3Body(draft), stored) : setTried(true))} />
        </View>
      </View>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SettingsCard>
  )
}

function StorageBar({ label, fraction }: { label: string; fraction: number | null }) {
  return (
    <View className="gap-2">
      <View className="flex-row justify-between gap-2">
        <Text className="text-[13px]">Used</Text>
        <Text className="text-[13px] text-muted-foreground tabular-nums">{label}</Text>
      </View>
      {fraction !== null ? (
        <View accessibilityRole="progressbar" accessibilityValue={{ min: 0, max: 100, now: Math.round(fraction * 100) }} className="h-1.5 rounded-full bg-muted overflow-hidden">
          <View className={`h-1.5 rounded-full ${fraction >= 0.9 ? 'bg-destructive' : 'bg-primary'}`} style={{ width: `${fraction * 100}%` }} />
        </View>
      ) : null}
    </View>
  )
}

/** Only admins get these from the server; they can delete a file (not one of an emoji or sticker). */
function LargestFiles({ files, page, loading, onPage }: { files: StorageFile[]; page: ReturnType<typeof storageFilesPage>; loading: boolean; onPage: (offset: number) => void }) {
  const deleteFile = useDeleteStorageFile()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  return (
    <View className="mt-5 gap-2">
      <Text className="text-[13px] font-medium">Largest files</Text>
      {deleteFile.isError ? <Note error>The file could not be deleted. {errorMessage(deleteFile.error)}</Note> : null}
      <View className="rounded-lg border border-border">
        {files.map((file, index) => {
          const { name, where } = storageFileLabel(file)
          return (
            <View key={file.id} className={`flex-row items-center gap-3 px-3 py-2 ${index > 0 ? 'border-t border-border' : ''}`}>
              <View className="flex-1 gap-0.5">
                <Text className="text-[13px]" numberOfLines={1}>{name}</Text>
                <Text className="text-xs text-muted-foreground" numberOfLines={1}>{where}</Text>
              </View>
              <Text className="text-[13px] text-muted-foreground tabular-nums">{formatBytes(file.byte_size)}</Text>
              {canDeleteStorageFile(file) ? (
                <Button
                  label="Delete"
                  variant="ghost"
                  size="sm"
                  disabled={deleteFile.isPending}
                  onPress={() =>
                    setConfirm({
                      title: `Delete ${name}?`,
                      description: 'It frees its storage at once. Tasks, pages and messages that use it show “Deleted file” instead. You cannot undo this.',
                      confirmLabel: 'Delete',
                      danger: true,
                      onConfirm: () => deleteFile.mutate(file.id),
                    })
                  }
                />
              ) : null}
            </View>
          )
        })}
      </View>
      {page.previous !== null || page.next !== null ? (
        <View className="flex-row items-center gap-2">
          <Text className="flex-1 text-xs text-muted-foreground tabular-nums">{page.label}</Text>
          <Button label="Previous" variant="outline" size="sm" disabled={page.previous === null || loading} onPress={() => page.previous !== null && onPage(page.previous)} />
          <Button label="Next" variant="outline" size="sm" disabled={page.next === null || loading} onPress={() => page.next !== null && onPage(page.next)} />
        </View>
      ) : null}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </View>
  )
}
