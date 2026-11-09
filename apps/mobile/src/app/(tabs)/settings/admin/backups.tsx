import { router } from 'expo-router'
import { useState } from 'react'
import { View } from 'react-native'

import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import { AdminShell, Badge, Note, SettingsCard, SettingsRow } from '../../../../components/settings/Shell'
import { backupSize } from '../../../../data/admin-api'
import { useBackupWrites, useBackups } from '../../../../hooks/admin'
import { errorMessage } from '../../../../session-context'
import { Button, Text } from '../../../../ui'

/**
 * The web's Admin → Backups: snapshots of the whole instance, created, browsed (to restore one page or task as a copy)
 * and deleted here. Downloading one stays on the web: a snapshot of every workspace and attachment is no file for a phone.
 */
export default function AdminBackupsScreen() {
  const backups = useBackups()
  const writes = useBackupWrites()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const list = [...(backups.data ?? [])].sort((a, b) => b.created_at - a.created_at)
  return (
    <AdminShell onRefresh={() => backups.refetch()}>
      <SettingsCard
        title="Backups"
        description="Create a verified snapshot of the database and attachments without stopping Orbit."
        actions={<Button label={writes.create.isPending ? 'Creating backup…' : 'Create backup now'} disabled={writes.create.isPending} onPress={() => writes.create.mutate()} />}
        flush
      >
        {writes.create.isError ? <View className="p-4 border-b border-border"><Note error>{errorMessage(writes.create.error)}</Note></View> : null}
        {writes.remove.isError ? <View className="p-4 border-b border-border"><Note error>The backup could not be deleted. {errorMessage(writes.remove.error)}</Note></View> : null}
        {backups.isPending ? <View className="p-4"><Note>Loading backups…</Note></View> : null}
        {backups.isError ? (
          <View className="p-4 flex-row items-center gap-2">
            <Note error>Backups could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void backups.refetch()} />
          </View>
        ) : null}
        {backups.data && list.length === 0 ? <View className="p-4"><Note>No backups yet.</Note></View> : null}
        {list.map((backup, index) => {
          const created = new Date(backup.created_at).toLocaleString()
          const deleting = writes.remove.isPending && writes.remove.variables === backup.id
          return (
            // the web's row: the date, badges and size beside the buttons, however narrow the text gets
            <SettingsRow key={backup.id} first={index === 0}>
              <View className="flex-1 min-w-0 gap-0.5">
                <View className="flex-row flex-wrap items-center gap-2">
                  <Text className="shrink font-noto-medium text-[13px] leading-[19.5px] text-foreground">{created}</Text>
                  {backup.kind === 'pre_migration' ? <Badge label="Before upgrade" tone="secondary" /> : null}
                  {backup.location === 's3' ? <Badge label="S3" tone="secondary" /> : null}
                </View>
                <Text className="text-[12px] leading-[16px] text-muted-foreground/70">
                  {`${backupSize(backup.byte_size)} · ${backup.file_count} ${backup.file_count === 1 ? 'file' : 'files'} · Orbit ${backup.application_version}`}
                </Text>
              </View>
              <Button label="Browse" icon="folderOpen" size="sm" variant="outline" onPress={() => router.push(`/settings/admin/backup/${backup.id}`)} />
              <Button
                label={deleting ? 'Deleting…' : 'Delete'}
                icon="trash"
                size="sm"
                variant="outline"
                disabled={deleting}
                onPress={() =>
                  setConfirm({
                    title: 'Delete this backup?',
                    description: `The backup from ${created} is removed for good. You cannot undo this.`,
                    confirmLabel: 'Delete',
                    danger: true,
                    onConfirm: () => writes.remove.mutate(backup.id),
                  })
                }
              />
            </SettingsRow>
          )
        })}
      </SettingsCard>
      <Text className="text-[12px] text-muted-foreground">Download backups from Orbit in a browser.</Text>
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </AdminShell>
  )
}
