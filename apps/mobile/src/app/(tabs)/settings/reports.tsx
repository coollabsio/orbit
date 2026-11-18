import { useQueryClient } from '@tanstack/react-query'
import { View } from 'react-native'

import { chatQueryKeys } from '../../../chat-cache'
import { useMessageText } from '../../../components/chat/MessageList'
import { REPORT_REASONS } from '../../../components/chat/Safety'
import { Badge, Note, SettingsCard, SettingsRow, SettingsShell, WORKSPACE_NAV } from '../../../components/settings/Shell'
import * as chat from '../../../data/chat-api'
import { relativeTime } from '../../../data/format'
import { confirmAction, useChatReports, useChatWrite } from '../../../hooks/chat'
import { useOrbit } from '../../../orbit-context'
import { Button, Text } from '../../../ui'

/** The chat reports of the workspace, newest first, for members who manage chat. Each can delete the reported message. */
export default function ReportsScreen() {
  const { workspace, workspaceId } = useOrbit()
  const allowed = workspace.permissions.includes('chat.manage')
  const reports = useChatReports(allowed)
  const run = useChatWrite()
  const queryClient = useQueryClient()
  const words = useMessageText()
  const list = reports.data ?? []

  const remove = async (report: chat.ChatReportRecord) => {
    if (!(await confirmAction('Delete message?', `This deletes the message from ${report.author_name} for everyone. This cannot be undone.`, 'Delete'))) return
    const done = await run('Could not delete the message', (client, workspaceId) => chat.deleteReportedMessage(client, workspaceId, report.id))
    if (done) await queryClient.invalidateQueries({ queryKey: chatQueryKeys.reports(workspaceId) })
  }

  return (
    <SettingsShell title="Settings" nav={WORKSPACE_NAV} onRefresh={allowed ? () => reports.refetch() : undefined}>
      <SettingsCard title="Reports" description="Chat messages that members reported. Read each report, then delete the message if it breaks your rules." flush>
        {!allowed ? <View className="p-4"><Note>Only members who manage chat can see reports.</Note></View> : null}
        {allowed && reports.isPending ? <View className="p-4"><Note loading>Loading reports…</Note></View> : null}
        {reports.isError ? <View className="p-4"><Note error>Reports could not be loaded.</Note></View> : null}
        {reports.isSuccess && list.length === 0 ? <View className="p-4"><Note>No reports.</Note></View> : null}
        {list.map((report, index) => (
          <SettingsRow key={report.id} first={index === 0}>
            <View className="flex-1 min-w-0 gap-1.5">
              <View className="flex-row flex-wrap items-center gap-2">
                <Badge label={REPORT_REASONS.find((reason) => reason.value === report.reason)?.label ?? report.reason} tone="secondary" />
                <Text className="text-[12px] leading-[16px] text-muted-foreground/70">{relativeTime(report.created_at)}</Text>
              </View>
              <Text className="text-[12px] leading-[16px] text-muted-foreground">
                <Text className="font-noto-medium text-[12px] leading-[16px] text-foreground">{report.reporter_name}</Text> reported a message from{' '}
                <Text className="font-noto-medium text-[12px] leading-[16px] text-foreground">{report.author_name}</Text>
              </Text>
              <View className="rounded-md border border-border bg-muted/50 px-3 py-2">
                <Text numberOfLines={6} className="text-[13px] leading-[19px] text-foreground/85">{words.text(report.body) || 'No text'}</Text>
              </View>
              {report.note ? <Text className="text-[13px] leading-[19px] text-muted-foreground">Note: {report.note}</Text> : null}
              <View className="flex-row items-center justify-end">
                {report.message_deleted || !report.message_id ? (
                  <Badge label="Message deleted" tone="outline" />
                ) : (
                  <Button label="Delete message" variant="destructive" size="sm" onPress={() => void remove(report)} />
                )}
              </View>
            </View>
          </SettingsRow>
        ))}
      </SettingsCard>
    </SettingsShell>
  )
}
