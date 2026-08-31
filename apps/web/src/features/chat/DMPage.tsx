import { useEffect, useState } from 'react'
import { Messages2 } from 'reicon-react'
import { useParams } from 'react-router'
import { EmptyState } from '../../components/ui/EmptyState'
import { markDirectMessageRead } from '../../mock/actions'
import { getState, useAppState } from '../../mock/store'
import type { ChatMessage } from '../../mock/types'
import { ChatArea } from './components/ChatArea'
import { DMSidebar } from './components/DMSidebar'
import { NewThreadPanel } from './components/NewThreadPanel'
import { ThreadPanel } from './components/ThreadPanel'
import './chat.css'

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
    <div className="page chat-page dm-page" data-view={channel ? 'conversation' : 'list'}>
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
        <div className="fc-chat-area">
          <EmptyState icon={Messages2} title="Your messages" description="Choose a conversation or start a new one." />
        </div>
      )}
    </div>
  )
}
