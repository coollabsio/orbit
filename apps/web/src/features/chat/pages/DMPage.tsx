import { useEffect, useState } from 'react'
import { Messages2 as MessagesSquare } from 'reicon-react'
import { useParams } from 'react-router'
import { EmptyState } from '@/components/common/EmptyState'
import { markDirectMessageRead } from '@/mock/actions'
import { getState, useAppState } from '@/mock/store'
import type { ChatMessage } from '@/mock/types'
import { ChatArea } from '@/features/chat/components/ChatArea'
import { DMSidebar } from '@/features/chat/components/DMSidebar'
import { NewThreadPanel } from '@/features/chat/components/NewThreadPanel'
import { ThreadPanel } from '@/features/chat/components/ThreadPanel'

type ThreadView = { kind: 'thread'; root: ChatMessage } | { kind: 'new' } | null

export function DMPage() {
  const { dmId } = useParams()
  const state = useAppState()
  const [threadView, setThreadView] = useState<ThreadView>(null)
  const dm = state.directMessages.find((candidate) => candidate.id === dmId) ?? null
  const participant = dm ? state.users.find((user) => user.id === dm.participantId) ?? null : null
  const channel = dm && participant ? {
    id: dm.id,
    name: participant.name,
    description: '',
    categoryId: 'direct-messages',
    unreadCount: dm.unreadCount,
  } : null

  useEffect(() => {
    if (dmId) markDirectMessageRead(dmId)
  }, [dmId])

  return (
    <div className="group/chat flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background" data-view={channel ? 'conversation' : 'list'}>
      <DMSidebar state={state} activeId={dmId ?? null} />
      {channel && participant ? (
        <>
          <ChatArea
            key={channel.id}
            state={state}
            channel={channel}
            dmParticipant={participant}
            membersOpen={false}
            onToggleMembers={() => {}}
            onOpenThread={(root) => setThreadView({ kind: 'thread', root })}
            onNewThread={() => setThreadView({ kind: 'new' })}
          />
          {threadView?.kind === 'new' ? (
            <NewThreadPanel
              state={state}
              channel={channel}
              onClose={() => setThreadView(null)}
              onCreated={(rootId) => {
                const root = getState().chatMessages.find((message) => message.id === rootId)
                setThreadView(root ? { kind: 'thread', root } : null)
              }}
            />
          ) : threadView?.kind === 'thread' ? (
            <ThreadPanel state={state} channel={channel} root={threadView.root} onClose={() => setThreadView(null)} />
          ) : null}
        </>
      ) : (
        <div className="relative flex min-w-0 flex-1 flex-col bg-background max-[899px]:group-data-[view=list]/chat:hidden">
          <EmptyState icon={MessagesSquare} title="Your messages" description="Choose a conversation or start a new one." />
        </div>
      )}
    </div>
  )
}
