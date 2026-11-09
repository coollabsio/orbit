import { router, useLocalSearchParams } from 'expo-router'
import { useEffect, useState } from 'react'
import { View } from 'react-native'

import { Dialog } from '../../../../../components/settings/Dialog'
import { AdminShell, Badge, Note, OptionHint, OptionText, RowIcon, RowText, Segmented, SettingsCard, SettingsRow, Toggle } from '../../../../../components/settings/Shell'
import { type BackupItem, type BackupItemKind, backupItemSubtitle, type RestoredItem, restoredRoute, restoredText, restoreItemBody } from '../../../../../data/admin-api'
import { useBackupItems, useBackups, useBackupWorkspaces, useRestoreBackupItem } from '../../../../../hooks/admin'
import { useOrbit } from '../../../../../orbit-context'
import { errorMessage } from '../../../../../session-context'
import { useTheme } from '../../../../../theme'
import { Button, Icon, Input, Spinner } from '../../../../../ui'

const KIND_OPTIONS: readonly { value: BackupItemKind; label: string }[] = [
  { value: 'page', label: 'Pages' },
  { value: 'task', label: 'Tasks' },
]

/**
 * At `backup/<id>`, not `backups/<id>`: the repo ignores every `backups/` folder.
 *
 * The web's backup browser: pick a workspace of the backup, find a page or task, and restore a copy of it into the
 * live workspace. Only workspaces that still exist can receive one.
 */
export default function AdminBackupBrowseScreen() {
  const { id: backupId } = useLocalSearchParams<{ id: string }>()
  const { colors } = useTheme()
  const { workspaceId: currentWorkspaceId, workspaces: memberOf, selectWorkspace } = useOrbit()
  const backup = useBackups().data?.find((item) => item.id === backupId)
  const workspaces = useBackupWorkspaces(backupId)
  const [chosen, setChosen] = useState<string | null>(null)
  const live = workspaces.data?.filter((workspace) => workspace.live) ?? []
  // the current workspace when the backup has it, else the first one that still exists
  const workspaceId = chosen ?? live.find((workspace) => workspace.id === currentWorkspaceId)?.id ?? live[0]?.id ?? null
  const [kind, setKind] = useState<BackupItemKind>('page')
  const [query, setQuery] = useState('')
  const [search, setSearch] = useState('')
  // the web waits 250 ms after the last keystroke before it searches
  useEffect(() => {
    const timer = setTimeout(() => setSearch(query.trim()), 250)
    return () => clearTimeout(timer)
  }, [query])
  const items = useBackupItems(backupId, workspaceId, kind, search)
  const restore = useRestoreBackupItem(backupId)
  const [confirming, setConfirming] = useState<BackupItem | null>(null)
  const [includeChildren, setIncludeChildren] = useState(true)
  const [restored, setRestored] = useState<{ item: RestoredItem; workspaceId: string } | null>(null)
  const restoredHref = restored ? restoredRoute(restored.item, restored.workspaceId, memberOf) : null

  const runRestore = (item: BackupItem) => {
    if (!workspaceId) return
    const target = workspaceId
    setRestored(null)
    restore.mutate({ workspaceId: target, body: restoreItemBody(item, includeChildren) }, { onSuccess: (copy) => setRestored({ item: copy, workspaceId: target }) })
  }

  const openRestored = () => {
    if (!restored || !restoredHref) return
    if (restored.workspaceId !== currentWorkspaceId) selectWorkspace(restored.workspaceId)
    router.push(restoredHref)
  }

  return (
    <AdminShell onRefresh={() => (workspaces.isError ? workspaces.refetch() : items.refetch())}>
      <SettingsCard
        title={backup ? `Backup from ${new Date(backup.created_at).toLocaleString()}` : 'Backup'}
        description="Restores a copy. Current data does not change. Comments and history are not restored."
        flush
      >
        {workspaces.isPending ? (
          <View className="p-4 flex-row items-center gap-2.5">
            <Spinner color={colors.mutedForeground} />
            <View className="flex-1">
              <Note>Opening the backup… The first open of a backup in S3 can take a while.</Note>
            </View>
          </View>
        ) : null}
        {workspaces.isError ? (
          <View className="p-4 flex-row items-center gap-2">
            <View className="flex-1">
              <Note error>{errorMessage(workspaces.error)}</Note>
            </View>
            <Button label="Retry" variant="ghost" onPress={() => void workspaces.refetch()} />
          </View>
        ) : null}
        {workspaces.data && workspaces.data.length === 0 ? <View className="p-4"><Note>This backup has no workspaces.</Note></View> : null}
        {workspaces.data?.map((workspace, index) => (
          <SettingsRow key={workspace.id} first={index === 0} label={workspace.name} onPress={workspace.live ? () => setChosen(workspace.id) : undefined}>
            <View className={`flex-1 flex-row items-center gap-3 ${workspace.live ? 'opacity-100' : 'opacity-50'}`}>
              <RowIcon icon="home" />
              <RowText title={workspace.name} badges={workspace.live ? undefined : <Badge label="Deleted" tone="secondary" />} />
              {workspace.id === workspaceId ? <Icon name="check" size={16} color={colors.foreground} /> : null}
            </View>
          </SettingsRow>
        ))}
      </SettingsCard>

      {workspaceId ? (
        <SettingsCard title={live.find((workspace) => workspace.id === workspaceId)?.name ?? 'Workspace'} flush>
          <View className="p-4 gap-3 border-b border-border">
            <Segmented label="Kind" value={kind} options={KIND_OPTIONS} onChange={setKind} />
            <Input value={query} onChangeText={setQuery} placeholder={kind === 'page' ? 'Search pages' : 'Search tasks'} autoCapitalize="none" autoCorrect={false} returnKeyType="search" />
            {restored ? (
              <View className="flex-row items-center gap-2">
                <View className="flex-1">
                  <Note>{restoredText(restored.item)}</Note>
                </View>
                {restoredHref ? <Button label="Open" size="sm" variant="outline" onPress={openRestored} /> : null}
              </View>
            ) : null}
            {restore.isError ? <Note error>{errorMessage(restore.error)}</Note> : null}
          </View>
          {items.isPending ? <View className="p-4"><Note>Loading…</Note></View> : null}
          {items.isError ? (
            <View className="p-4 flex-row items-center gap-2">
              <View className="flex-1">
                <Note error>{errorMessage(items.error)}</Note>
              </View>
              <Button label="Retry" variant="ghost" onPress={() => void items.refetch()} />
            </View>
          ) : null}
          {items.data && items.data.length === 0 ? <View className="p-4"><Note>{search ? 'Nothing matches this search.' : kind === 'page' ? 'No pages in this backup.' : 'No tasks in this backup.'}</Note></View> : null}
          {items.data?.map((item, index) => {
            const restoring = restore.isPending && restore.variables?.body.id === item.id
            return (
              <SettingsRow key={item.id} first={index === 0}>
                <RowIcon icon={item.kind === 'page' ? 'docs' : 'tasks'} />
                <RowText title={item.title || 'Untitled'} subtitle={backupItemSubtitle(item)} badges={item.deleted ? <Badge label="In trash" tone="secondary" /> : null} />
                <Button
                  label={restoring ? 'Restoring…' : 'Restore copy'}
                  size="sm"
                  variant="outline"
                  disabled={restore.isPending}
                  onPress={() => {
                    setIncludeChildren(true)
                    setConfirming(item)
                  }}
                />
              </SettingsRow>
            )
          })}
        </SettingsCard>
      ) : null}

      <Dialog
        visible={confirming !== null}
        title={`Restore a copy of ${confirming?.title || 'Untitled'}?`}
        description={`A new ${confirming?.kind === 'task' ? 'task' : 'page'} is created from the backup. Current data does not change.`}
        onClose={() => setConfirming(null)}
        footer={
          <>
            <Button label="Cancel" variant="outline" onPress={() => setConfirming(null)} />
            <Button
              label="Restore copy"
              onPress={() => {
                const item = confirming
                setConfirming(null)
                if (item) runRestore(item)
              }}
            />
          </>
        }
      >
        {confirming?.kind === 'page' && confirming.child_count > 0 ? (
          <View className="flex-row items-center gap-3">
            <OptionText title="Include sub-pages">
              <OptionHint>{`${confirming.child_count} ${confirming.child_count === 1 ? 'sub-page' : 'sub-pages'}`}</OptionHint>
            </OptionText>
            <Toggle label="Include sub-pages" value={includeChildren} onChange={setIncludeChildren} />
          </View>
        ) : null}
      </Dialog>
    </AdminShell>
  )
}
