import { useState } from 'react'
import { Hashtag } from 'reicon-react'
import { useParams } from 'react-router'
import { EmptyState } from '../../components/ui/EmptyState'
import { useAppState } from '../../mock/store'
import { ChannelSidebar } from './components/ChannelSidebar'
import { ChatArea } from './components/ChatArea'
import { MemberList } from './components/MemberList'
import './chat.css'

function isMobileViewport() {
  return window.matchMedia('(max-width: 899px)').matches
}

export function ChatPage() {
  const { channelId } = useParams()
  const state = useAppState()
  const [membersOpen, setMembersOpen] = useState(true)
  const [mobileMembersOpen, setMobileMembersOpen] = useState(false)
  const channel = state.channels.find((c) => c.id === channelId) ?? null

  const toggleMembers = () => {
    if (isMobileViewport()) setMobileMembersOpen((o) => !o)
    else setMembersOpen((o) => !o)
  }

  return (
    <div className="page chat-page" data-view={channel ? 'conversation' : 'list'}>
      <ChannelSidebar state={state} activeChannelId={channel?.id ?? null} />
      {channel ? (
        <>
          <ChatArea
            key={channel.id}
            state={state}
            channel={channel}
            membersOpen={membersOpen}
            onToggleMembers={toggleMembers}
          />
          {membersOpen ? <MemberList state={state} /> : null}
          {mobileMembersOpen ? (
            <MemberList state={state} isMobile onClose={() => setMobileMembersOpen(false)} />
          ) : null}
        </>
      ) : (
        <div className="fc-chat-area">
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
