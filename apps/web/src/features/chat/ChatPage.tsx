import { useEffect, useState } from 'react'
import { Hashtag } from 'reicon-react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { EmptyState } from '../../components/ui/EmptyState'
import { useAppState } from '../../mock/store'
import type { ChatMessage } from '../../mock/types'
import { ChannelSidebar } from './components/ChannelSidebar'
import { ChatArea } from './components/ChatArea'
import { MemberList } from './components/MemberList'
import { NewThreadPanel } from './components/NewThreadPanel'
import { ThreadPanel } from './components/ThreadPanel'
import './chat.css'

function isMobileViewport() {
  return window.matchMedia('(max-width: 899px)').matches
}

/** Thread side pane state, scoped to a channel so it closes on channel switch. */
type ThreadView = { channelId: string; kind: 'thread'; rootId: string } | { channelId: string; kind: 'new' }

export function ChatPage() {
  const { channelId, rootId } = useParams()
  const [searchParams] = useSearchParams()
  const state = useAppState()
  const [membersOpen, setMembersOpen] = useState(true)
  const [mobileMembersOpen, setMobileMembersOpen] = useState(false)
  // the chat reference deep link: /chat/:channelId?thread=<rootId> opens the thread pane
  const [threadView, setThreadView] = useState<ThreadView | null>(() => {
    const rootId = searchParams.get('thread')
    return channelId && rootId ? { channelId, kind: 'thread', rootId } : null
  })
  const navigate = useNavigate()
  const firstChannel = state.channels[0] ?? null
  // no channel in the URL: show the first channel right away (the chat reference picks the first channel)
  const channel = channelId ? (state.channels.find((c) => c.id === channelId) ?? null) : firstChannel

  const toggleMembers = () => {
    if (isMobileViewport()) setMobileMembersOpen((o) => !o)
    else setMembersOpen((o) => !o)
  }

  const activeThread = channel && threadView?.channelId === channel.id ? threadView : null
  const threadRoot: ChatMessage | null =
    activeThread?.kind === 'thread' ? (state.chatMessages.find((m) => m.id === activeThread.rootId) ?? null) : null
  const mobile = isMobileViewport()
  // the chat reference full-screen thread: /chat/:channelId/thread/:rootId replaces the chat area
  const fullScreenRoot: ChatMessage | null = rootId ? (state.chatMessages.find((m) => m.id === rootId) ?? null) : null

  const threadPane =
    channel && activeThread ? (
      activeThread.kind === 'new' ? (
        <NewThreadPanel
          state={state}
          channel={channel}
          onClose={() => setThreadView(null)}
          onCreated={(rootId) => setThreadView({ channelId: channel.id, kind: 'thread', rootId })}
        />
      ) : threadRoot ? (
        <ThreadPanel state={state} channel={channel} root={threadRoot} onClose={() => setThreadView(null)} isMobile={mobile} />
      ) : null
    ) : null

  useEffect(() => {
    if (!channelId && firstChannel) navigate(`/chat/${firstChannel.id}`, { replace: true })
  }, [channelId, firstChannel, navigate])

  return (
    <div className="page chat-page" data-view={channel ? 'conversation' : 'list'}>
      <ChannelSidebar
        state={state}
        activeChannelId={channel?.id ?? null}
        activeThreadId={fullScreenRoot ? fullScreenRoot.id : activeThread?.kind === 'thread' ? activeThread.rootId : null}
        onOpenThread={(channelId, rootId) => {
          setThreadView({ channelId, kind: 'thread', rootId })
          if (channelId !== channel?.id || fullScreenRoot) navigate(`/chat/${channelId}`)
        }}
      />
      {channel && fullScreenRoot ? (
        <ThreadPanel state={state} channel={channel} root={fullScreenRoot} onClose={() => navigate(`/chat/${channel.id}`)} fullScreen />
      ) : channel ? (
        <>
          <ChatArea
            key={channel.id}
            state={state}
            channel={channel}
            membersOpen={membersOpen}
            onToggleMembers={toggleMembers}
            onOpenThread={(root) => setThreadView({ channelId: channel.id, kind: 'thread', rootId: root.id })}
            onNewThread={() => setThreadView({ channelId: channel.id, kind: 'new' })}
            rightPanel={membersOpen && !threadPane && !mobile ? <MemberList state={state} /> : undefined}
          />
          {threadPane ? (
            mobile ? (
              <>
                <div className="fc-members-mobile-backdrop" onClick={() => setThreadView(null)} />
                <div className="fc-thread-mobile">{threadPane}</div>
              </>
            ) : (
              threadPane
            )
          ) : null}
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
