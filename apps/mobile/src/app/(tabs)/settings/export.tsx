import type { WorkspaceExport } from '@orbit/api/generated/types.gen'
import { File, Paths } from 'expo-file-system'
import { useState } from 'react'
import { Alert, Platform, Share, View } from 'react-native'

import { SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { exportDownloadPath, formatSize, isActiveExport } from '../../../data/workspace-settings-api'
import { useStartWorkspaceExport, useWorkspaceExports } from '../../../hooks/workspace-settings'
import { useOrbit } from '../../../orbit-context'
import { errorMessage } from '../../../session-context'
import { useTheme } from '../../../theme'
import { Button, Spinner, Text } from '../../../ui'

const TITLE = 'Export workspace'

/** A ZIP of the whole workspace (tasks, pages, chat and files) to download and keep. */
export default function ExportScreen() {
  const { workspace } = useOrbit()
  const canExport = workspace.permissions.includes('workspace.export')
  const exports = useWorkspaceExports(canExport)
  const start = useStartWorkspaceExport()
  const active = exports.data?.some(isActiveExport) ?? false
  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={canExport ? () => exports.refetch() : undefined}>
      {canExport ? (
        <SettingsCard
          title={TITLE}
          description="Download everything in this workspace as one ZIP file: tasks with comments, pages as Markdown, chat and all attached files. It includes your private pages and the private channels and direct messages you are in, so only you see and download your exports. Each download is kept for 7 days."
          actions={
            <Button
              label={active ? 'Export in progress' : 'Start export'}
              disabled={active || start.isPending || !exports.isSuccess}
              onPress={() => start.mutate(undefined, { onError: (error) => Alert.alert('The export could not be started.', errorMessage(error)) })}
            />
          }
          flush
        >
          {exports.isError ? (
            <SettingsRow first>
              <Text className="flex-1 text-[13px]">Exports could not be loaded.</Text>
              <Button label="Retry" variant="ghost" onPress={() => void exports.refetch()} />
            </SettingsRow>
          ) : null}
          {exports.data?.length === 0 ? (
            <View className="px-4 py-3">
              <Text className="text-[13px] leading-[19.5px] text-muted-foreground">No exports yet.</Text>
            </View>
          ) : null}
          {exports.data?.map((item, index) => <ExportRow key={item.id} item={item} first={index === 0} />)}
        </SettingsCard>
      ) : (
        <SettingsCard title={TITLE}>
          <Text className="text-[14px] leading-[20px] text-muted-foreground">Only workspace owners and administrators can export the workspace.</Text>
        </SettingsCard>
      )}
    </SettingsShell>
  )
}

function ExportRow({ item, first }: { item: WorkspaceExport; first: boolean }) {
  const { colors } = useTheme()
  const created = new Date(item.created_at).toLocaleString()
  const expires = new Date(item.expires_at).toLocaleString()
  const download = useDownload(item)
  return (
    <SettingsRow first={first}>
      <View className="min-w-0 flex-1 gap-0.5">
        <Text className="font-noto-medium text-[13px] text-foreground">{created}</Text>
        {item.status === 'ready' ? (
          <Text className="text-[12px] text-muted-foreground/70">
            {formatSize(item.size_bytes ?? 0)} · Deleted on {expires}
          </Text>
        ) : item.status === 'failed' ? (
          <Text className="text-[12px] text-destructive">The export failed. Start a new one.</Text>
        ) : (
          <View className="flex-row items-center gap-1.5">
            <View className="size-3.5 scale-70 items-center justify-center"><Spinner color={colors.mutedForeground} /></View>
            <Text className="text-[12px] text-muted-foreground">{item.status === 'queued' ? 'Waiting to start…' : 'Preparing the archive…'}</Text>
          </View>
        )}
      </View>
      {item.status === 'ready' ? <Button label="Download" icon="download" variant="outline" size="sm" loading={download.busy} onPress={() => void download.run()} /> : null}
    </SettingsRow>
  )
}

/**
 * Downloads the ZIP with the session token (never in the URL). The app saves it to the cache and opens the system preview
 * (or the share sheet), where it can be saved or shared, as a chat file opens; the Expo web build hands the browser the file, as the web's download link does.
 */
function useDownload(item: WorkspaceExport) {
  const { session, workspaceId } = useOrbit()
  const [busy, setBusy] = useState(false)
  const url = `${session.server}${exportDownloadPath(workspaceId, item.id)}`
  const name = `orbit-export-${item.created_at.slice(0, 10)}.zip`
  const run = async () => {
    setBusy(true)
    try {
      if (Platform.OS === 'web') {
        const response = await fetch(url, { headers: { Authorization: `Bearer ${session.token}` } })
        if (!response.ok) throw new Error(`Download failed (${response.status})`)
        const link = document.createElement('a')
        link.href = URL.createObjectURL(await response.blob())
        link.download = name
        link.click()
        return
      }
      const target = new File(Paths.cache, `export-${item.id}.zip`)
      const file = target.exists ? target : await File.downloadFileAsync(url, target, { headers: { Authorization: `Bearer ${session.token}` }, idempotent: true })
      if (await file.canPreview()) await file.preview()
      else await Share.share({ url: file.uri, title: name })
    } catch (error) {
      Alert.alert('Could not download the export', errorMessage(error))
    } finally {
      setBusy(false)
    }
  }
  return { busy, run }
}
