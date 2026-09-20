import { useEffect, useState } from 'react'
import { Hash } from 'lucide-react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { EmptyState } from '@/components/common/EmptyState'
import { useAppState } from '@/mock/store'
import type { ChatMessage } from '@/mock/types'
import { ChannelSidebar } from '@/features/chat/components/ChannelSidebar'
import { ChatArea } from '@/features/chat/components/ChatArea'
import { MemberList } from '@/features/chat/components/MemberList'
import { NewThreadPanel } from '@/features/chat/components/NewThreadPanel'
import { ThreadPanel } from '@/features/chat/components/ThreadPanel'

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
  const mobile = isMobileViewport()
  // Desktop opens the first channel by default; mobile keeps /chat as the channel list.
  const channel = channelId ? (state.channels.find((c) => c.id === channelId) ?? null) : mobile ? null : firstChannel

  const toggleMembers = () => {
    if (isMobileViewport()) setMobileMembersOpen((o) => !o)
    else setMembersOpen((o) => !o)
  }

  const activeThread = channel && threadView?.channelId === channel.id ? threadView : null
  const threadRoot: ChatMessage | null =
    activeThread?.kind === 'thread' ? (state.chatMessages.find((m) => m.id === activeThread.rootId) ?? null) : null
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
    if (!mobile && !channelId && firstChannel) navigate(`/chat/${firstChannel.id}`, { replace: true })
  }, [channelId, firstChannel, mobile, navigate])

  return (
    <div className="group/chat flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background" data-view={channel ? 'conversation' : 'list'}>
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
                <div className="fixed inset-0 z-40 bg-black/50" onClick={() => setThreadView(null)} />
                <div className="fixed top-0 right-0 bottom-0 z-50 w-[min(320px,100vw)] duration-200 animate-in slide-in-from-right motion-reduce:animate-none">{threadPane}</div>
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
        <div className="relative flex min-w-0 flex-1 flex-col bg-background max-[899px]:group-data-[view=list]/chat:hidden">
          <EmptyState
            icon={Hash}
            title="Pick a channel"
            description="Choose a channel from the list to start reading and chatting."
          />
        </div>
      )}
    </div>
  )
}
