import { useState } from 'react'
import { Hashtag } from 'reicon-react'
import { useParams } from 'react-router'
import { EmptyState } from '../../components/ui/EmptyState'
import { useAppState } from '../../mock/store'
import { ChannelRail } from './components/ChannelRail'
import { Conversation } from './components/Conversation'
import { MemberPanel } from './components/MemberPanel'
import './chat.css'

export function ChatPage() {
  const { channelId } = useParams()
  const state = useAppState()
  const [membersOpen, setMembersOpen] = useState(true)
  const channel = state.channels.find((c) => c.id === channelId) ?? null

  return (
    <div className="page chat-page" data-view={channel ? 'conversation' : 'list'}>
      <ChannelRail state={state} activeChannelId={channel?.id ?? null} />
      {channel ? (
        <>
          <Conversation
            key={channel.id}
            state={state}
            channel={channel}
            membersOpen={membersOpen}
            onToggleMembers={() => setMembersOpen((o) => !o)}
          />
          {membersOpen ? <MemberPanel state={state} /> : null}
        </>
      ) : (
        <div className="pane chat-conversation">
          <EmptyState
            icon={Hashtag}
            title="Pick a channel"
            description="Choose a channel from the list to start reading and chatting."
          />
        </div>
      )}
    </div>
  )
}
