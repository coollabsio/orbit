import { Flag } from 'reicon-react'
import { toast } from 'sonner'
import type { ChatReportRecord } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { EmptyState } from '@/components/common/EmptyState'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { LoadingText } from '@/components/ui/spinner'
import { REPORT_REASONS, useChatReports, useDeleteReportedMessage } from '@/features/chat/api/moderation'
import { useConversations } from '@/features/chat/api/queries'
import { decodeMentions } from '@/features/chat/lib/mentionTokens'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useMembers } from '@/features/workspaces/api'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { relativeTime } from '@/lib/format'

/** The chat messages that members reported, newest first, for the members who manage chat (`chat.manage`). */
export function ReportsPage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('chat.manage')
  const reports = useChatReports(workspace.id, canManage)
  const members = useMembers(canManage ? workspace.id : null).data ?? []
  const conversations = useConversations().data ?? []
  const deleteMessage = useDeleteReportedMessage(workspace.id)

  if (!canManage) {
    return (
      <SettingsCard title="Reports">
        <p className="m-0 text-muted-foreground">Only workspace owners and admins can see the reports.</p>
      </SettingsCard>
    )
  }

  async function remove(report: ChatReportRecord) {
    const confirmed = await confirmAction({
      title: 'Delete message?',
      description: `This deletes the message of ${report.author_name} for everyone. You cannot undo this.`,
      confirmLabel: 'Delete',
      danger: true,
    })
    if (!confirmed) return
    deleteMessage.mutate(report.id, {
      onSuccess: () => toast.success('Message deleted'),
      onError: () => toast.error('Could not delete the message. Try again.'),
    })
  }

  return (
    <SettingsCard title="Reports" description="Chat messages that members reported. Deleting a message removes it for everyone, also in direct messages." flush>
      <div className="flex flex-col divide-y">
        {reports.isPending ? <SettingsRow><LoadingText>Loading reports…</LoadingText></SettingsRow> : null}
        {reports.isError ? <SettingsRow role="alert">Reports could not be loaded. <Button variant="ghost" onClick={() => void reports.refetch()}>Retry</Button></SettingsRow> : null}
        {reports.data?.length === 0 ? <EmptyState size="sm" icon={Flag} title="No reports" description="When a member reports a chat message, it shows here." /> : null}
        {reports.data?.map((report) => (
          <SettingsRow key={report.id} data-slot="chat-report" className="flex-nowrap items-start">
            <div className="flex min-w-0 flex-1 flex-col gap-1.5">
              <span className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[0.8125rem]">
                <span className="font-medium text-foreground">{report.author_name}</span>
                <Badge variant="secondary">{REPORT_REASONS[report.reason]}</Badge>
                <span className="text-xs text-muted-foreground/70" title={new Date(report.created_at).toLocaleString()}>
                  Reported by {report.reporter_name} · {relativeTime(report.created_at)}
                </span>
              </span>
              <p className="m-0 max-h-40 overflow-y-auto rounded-md bg-muted/50 px-3 py-2 text-[0.8125rem] wrap-anywhere whitespace-pre-wrap text-foreground">
                {decodeMentions(report.body, members, conversations) || 'No text'}
              </p>
              {report.note ? <p className="m-0 text-xs wrap-anywhere whitespace-pre-wrap text-muted-foreground">Note: {report.note}</p> : null}
            </div>
            {report.message_deleted || !report.message_id ? (
              <Badge variant="outline" className="shrink-0">Message deleted</Badge>
            ) : (
              <Button type="button" variant="ghost" className="shrink-0" disabled={deleteMessage.isPending} onClick={() => void remove(report)}>
                Delete message
              </Button>
            )}
          </SettingsRow>
        ))}
      </div>
    </SettingsCard>
  )
}
