import { Messages2 } from 'reicon-react'
import { UserAvatarStack } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useConversationThreads } from '@/features/chat/api/queries'
import { MessageBody } from '@/features/chat/components/messages/MessageBody'
import { relativeAgo } from '@/features/chat/lib/time'
import { useChatNavigation } from '@/features/chat/useChatNavigation'
import { useMembers } from '@/features/workspaces/api'
import { MessageByline } from './MessageByline'
import { RightPane, RightPaneBody, RightPaneEmpty, RightPaneError, RightPaneLoading } from './RightPane'

/** Every thread of a conversation, the one with the newest reply first. */
export function ThreadsPane({ conversationId }: { conversationId: string }) {
  const { workspaceId } = useChatContext()
  const threads = useConversationThreads(conversationId)
  const members = useMembers(workspaceId).data ?? []
  const { openThreadPane } = useChatNavigation()
  const roots = [...(threads.data ?? [])].sort((a, b) => (b.lastReplyAt ?? b.createdAt) - (a.lastReplyAt ?? a.createdAt))

  return (
    <RightPane title="Threads">
      <RightPaneBody>
        {threads.isPending ? (
          <RightPaneLoading />
        ) : threads.isError ? (
          <RightPaneError title="The threads could not be loaded." onRetry={() => void threads.refetch()} />
        ) : roots.length === 0 ? (
          <RightPaneEmpty icon={Messages2} title="No threads yet" description="Reply to a message to start one." />
        ) : (
          <ul aria-label="Threads" className="flex flex-col gap-1">
            {roots.map((root) => {
              const repliers = root.replyUserIds.slice(0, 3).flatMap((id) => members.find((member) => member.id === id) ?? [])
              return (
                <li key={root.id} className="flex flex-col gap-1 rounded-lg p-2 hover-fine:hover:bg-muted/50">
                  <MessageByline message={root} members={members} />
                  {/* a long root shows its first lines only; the thread shows all of it */}
                  <div className="max-h-28 overflow-hidden">
                    <MessageBody message={root} />
                  </div>
                  <Button variant="ghost" size="sm" className="-ml-2 w-fit max-w-full gap-2" onClick={() => openThreadPane(conversationId, root.id)}>
                    {repliers.length > 0 ? <UserAvatarStack users={repliers} size={18} max={3} /> : null}
                    <span>{root.replyCount === 1 ? '1 reply' : `${root.replyCount} replies`}</span>
                    {root.lastReplyAt ? <span className="truncate font-normal text-muted-foreground">Last reply {relativeAgo(root.lastReplyAt)}</span> : null}
                  </Button>
                </li>
              )
            })}
          </ul>
        )}
      </RightPaneBody>
    </RightPane>
  )
}
