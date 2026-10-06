import { useEffect, useState, type ReactNode } from 'react'
import { Navigate, useNavigate } from 'react-router'
import { Lock, Message } from 'reicon-react'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane } from '@/components/common/Pane'
import { SideSheet, SideSheetContent } from '@/components/common/SideSheet'
import { Button } from '@/components/ui/button'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useCategories, useConversations, useConversationStates } from '@/features/chat/api/queries'
import type { Message as ChatMessage } from '@/features/chat/api/types'
import { ChatHostContext, type ChatHost } from '@/features/chat/chatHost'
import { conversationPath, messagePath, UNREADS_PATH } from '@/features/chat/chatRoutes'
import { ConversationHeader } from '@/features/chat/components/ConversationHeader'
import { ConversationView } from '@/features/chat/components/ConversationView'
import { BrowseChannelsDialog } from '@/features/chat/components/dialogs/BrowseChannelsDialog'
import { FilesPane } from '@/features/chat/components/panes/FilesPane'
import { MembersPane } from '@/features/chat/components/panes/MembersPane'
import { PinsPane } from '@/features/chat/components/panes/PinsPane'
import { SearchPane } from '@/features/chat/components/panes/SearchPane'
import { ThreadsPane } from '@/features/chat/components/panes/ThreadsPane'
import { ChatSidebar } from '@/features/chat/components/sidebar/ChatSidebar'
import { ResizeHandle } from '@/features/chat/components/sidebar/ResizeHandle'
import { adjacentConversation, sidebarOrder } from '@/features/chat/components/sidebar/sidebarRows'
import { useStoredWidth } from '@/features/chat/components/sidebar/storedWidth'
import { ThreadView } from '@/features/chat/components/thread/ThreadView'
import { ThreadsView } from '@/features/chat/components/views/ThreadsView'
import { UnreadsView } from '@/features/chat/components/views/UnreadsView'
import { useMarkAllReadWithUndo } from '@/features/chat/components/views/useMarkAllReadWithUndo'
import { chatThemeVariables, useChatTheme } from '@/features/chat/lib/chatTheme'
import { getLastOpenedConversation, setLastOpenedConversation } from '@/features/chat/lib/drafts'
import { decodeMentions } from '@/features/chat/lib/mentionTokens'
import { buildSidebarSections, conversationBadge } from '@/features/chat/lib/sidebar'
import { useChatLocation, useChatNavigation, useOpenPane } from '@/features/chat/useChatNavigation'
import { useProjects } from '@/features/tasks/api/projects'
import { useOpenNewTask } from '@/features/tasks/newTask'
import { useMembers } from '@/features/workspaces/api'
import { useBindings, useCommand, useRunCommand } from '@/shortcuts/useCommand'
import { ChatLinkCard, ChatLinkChip } from './ChatLinkCard'
import { homeConversation, orbitLinkTarget, PANE_WIDTH, paneLayout, rightPaneOf, taskTitleFromMessage, type RightPane } from './chatPageLib'
import { openedByPointer, useElementWidth, useInputModality, useIsPhone } from './useChatViewport'
import { loadFailed } from '@/lib/connection'

const PANE_LABEL: Record<RightPane['kind'], string> = { thread: 'Thread', search: 'Search', members: 'Members', pins: 'Pinned messages', files: 'Files', threads: 'Threads' }

/** The content of the right pane. Each pane brings its own 48px header, close button and accessible name. */
function PaneContent({ conversationId, pane }: { conversationId: string; pane: RightPane }) {
  switch (pane.kind) {
    case 'thread':
      return <ThreadView key={pane.rootId} conversationId={conversationId} rootId={pane.rootId} variant="pane" focusMessageId={pane.focusMessageId ?? undefined} />
    case 'search':
      return <SearchPane conversationId={conversationId} query={pane.query} />
    case 'members':
      return <MembersPane conversationId={conversationId} />
    case 'pins':
      return <PinsPane conversationId={conversationId} />
    case 'files':
      return <FilesPane conversationId={conversationId} />
    case 'threads':
      return <ThreadsPane conversationId={conversationId} />
  }
}

/**
 * The right pane beside the message column. It mounts when a pane opens, so "opened by the pointer" is read once:
 * then it fades in (opacity only, no transform: a transform would re-parent fixed children). A key opens it at once,
 * and it never animates out.
 */
function InlinePane({ width, onResize, children }: { width: number; onResize: (width: number) => void; children: ReactNode }) {
  const [fade] = useState(openedByPointer)
  return (
    <div
      data-slot="chat-pane-frame"
      data-fade={fade || undefined}
      className="relative flex min-h-0 shrink-0 flex-col border-l bg-background motion-reduce:transition-none data-fade:transition-opacity data-fade:duration-150 data-fade:ease-out data-fade:starting:opacity-0"
      style={{ width }}
    >
      <ResizeHandle edge="left" width={width} range={PANE_WIDTH} label="Resize the right pane" onResize={onResize} />
      {children}
    </div>
  )
}

/** A pane as a sheet from the right: on a phone, and when the window cannot fit the pane beside the message column. */
function PaneSheet({ conversationId, pane, onClose }: { conversationId: string | null; pane: RightPane | null; onClose: () => void }) {
  // the last pane stays rendered while the sheet plays its exit
  const key = pane && conversationId ? `${conversationId}:${JSON.stringify(pane)}` : null
  const [kept, setKept] = useState<{ key: string; conversationId: string; pane: RightPane } | null>(null)
  if (key && pane && conversationId && kept?.key !== key) setKept({ key, conversationId, pane })
  return (
    <SideSheet
      open={key !== null}
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      <SideSheetContent
        side="right"
        aria-label={kept ? PANE_LABEL[kept.pane.kind] : 'Pane'}
        className="w-[min(420px,100vw)] border-l bg-card pt-[env(safe-area-inset-top,0px)] pb-[env(safe-area-inset-bottom,0px)]"
      >
        {kept ? <PaneContent conversationId={kept.conversationId} pane={kept.pane} /> : null}
      </SideSheetContent>
    </SideSheet>
  )
}

const OPEN_POPUP = '[role="menu"]:not([data-closed]), [role="dialog"]:not([data-closed]), [role="alertdialog"]:not([data-closed])'

/** The chat shortcuts. They are active while the chat page is mounted, also with the focus in the composer. */
function ChatCommands() {
  const location = useChatLocation()
  const navigation = useChatNavigation()
  const navigate = useNavigate()
  const conversations = useConversations().data
  const categories = useCategories().data
  const states = useConversationStates().data
  const markAllRead = useMarkAllReadWithUndo()
  const bindings = useBindings()
  const runCommand = useRunCommand()

  const ids = sidebarOrder(buildSidebarSections(conversations ?? [], categories ?? [], states ?? []))
  const currentId = location.view === 'conversation' || location.view === 'thread' ? location.conversationId : null
  const isUnread = (id: string) => {
    const conversation = conversations?.find((candidate) => candidate.id === id)
    if (!conversation) return false
    const badge = conversationBadge(conversation, states?.find((state) => state.conversationId === id))
    return badge.bold || badge.count > 0
  }
  const go = (direction: 'previous' | 'next', matches?: (id: string) => boolean) => {
    const id = adjacentConversation(ids, currentId, direction, matches)
    if (id) navigation.openConversation(id)
  }

  useCommand('chat.previous', () => go('previous'))
  useCommand('chat.next', () => go('next'))
  useCommand('chat.previousUnread', () => go('previous', isUnread))
  useCommand('chat.nextUnread', () => go('next', isUnread))
  useCommand('chat.unreads', () => navigate(UNREADS_PATH))
  useCommand('chat.markAllRead', markAllRead)

  // The key engine gives a text field every key without Ctrl, ⌘ or Alt, so `Shift+Esc` would not reach the command
  // while the composer has the focus. This listener covers that one case, for the default keys only.
  const markAllKeys = bindings['chat.markAllRead']
  useEffect(() => {
    if (markAllKeys !== 'Shift+Escape') return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || !event.shiftKey || event.ctrlKey || event.metaKey || event.altKey || event.repeat) return
      const target = event.target
      const editable = target instanceof HTMLElement && (target.matches('input, textarea, select') || target.isContentEditable)
      if (!editable || document.querySelector(OPEN_POPUP)) return
      // also keeps the local `Esc` handlers (close the pane, cancel an edit) from acting on this key
      event.preventDefault()
      event.stopPropagation()
      runCommand('chat.markAllRead')
    }
    window.addEventListener('keydown', onKeyDown, true)
    return () => window.removeEventListener('keydown', onKeyDown, true)
  }, [markAllKeys, runCommand])

  return null
}

/**
 * The chat page for every chat URL: chat sidebar | main column | right pane.
 * Desktop: flat panes divided by 1px borders; `/chat` goes to the last opened conversation.
 * Phone: `/chat` is the list; a conversation, a view or a thread is a full screen; panes are sheets.
 * Nothing here is transformed or animated except the opacity of a pane that the pointer opened.
 */
export function ChatPage() {
  const location = useChatLocation()
  const theme = useChatTheme()
  const navigation = useChatNavigation()
  const openPane = useOpenPane()
  const { workspaceId } = useChatContext()
  const conversations = useConversations()
  const people = useMembers(workspaceId).data
  const openNewTask = useOpenNewTask()
  const projects = useProjects(workspaceId).data
  const phone = useIsPhone()
  const [areaRef, areaWidth] = useElementWidth()
  const [paneWidth, setPaneWidth] = useStoredWidth('orbit:chat:pane_width', PANE_WIDTH)
  const [browseOpen, setBrowseOpen] = useState(false)
  useInputModality()

  const conversationId = location.view === 'conversation' || location.view === 'thread' ? location.conversationId : null
  const conversation = conversationId ? conversations.data?.find((candidate) => candidate.id === conversationId) : undefined
  const openId = conversation?.id
  useEffect(() => {
    if (openId) setLastOpenedConversation(workspaceId, openId)
  }, [openId, workspaceId])

  const host: ChatHost = {
    renderLinkCard: (url) => {
      const target = orbitLinkTarget(url, window.location.origin)
      return target ? <ChatLinkCard kind={target.kind} id={target.id} /> : null
    },
    renderLink: (url, plain, hidden) => {
      const target = orbitLinkTarget(url, window.location.origin)
      return target ? <ChatLinkChip kind={target.kind} id={target.id} hidden={hidden}>{plain}</ChatLinkChip> : plain
    },
    taskKeys: (projects ?? []).map((project) => project.key),
    createTask: (message: ChatMessage) => {
      const source = conversations.data?.find((candidate) => candidate.id === message.conversationId)
      const text = decodeMentions(message.body, people ?? [], conversations.data ?? [])
      const where = source && source.kind !== 'dm' ? `a message in #${source.name}` : 'a direct message'
      openNewTask({
        title: taskTitleFromMessage(text),
        description: `Created from [${where}](${window.location.origin}${messagePath(message)}).`,
      })
    },
  }

  if (location.view === 'home' && !phone && conversations.data) {
    const target = homeConversation(conversations.data, getLastOpenedConversation(workspaceId))
    if (target) return <Navigate to={conversationPath(target.id)} replace />
  }

  // the URL's pane, else the pane the user left open: it stays beside every conversation until they close it
  const urlPane = rightPaneOf(location)
  const pane = urlPane ?? (openPane ? { kind: openPane } : null)
  const layout = paneLayout(areaWidth, paneWidth)
  const fits = !phone && layout.fits
  const inlinePane = conversation && pane && fits ? pane : null
  // a sheet covers the conversation, so only the URL opens one
  const sheetPane = conversation && urlPane && !fits && urlPane.kind !== 'thread' ? urlPane : null
  const browseAction = (
    <Button type="button" onClick={() => setBrowseOpen(true)}>
      Browse channels
    </Button>
  )

  let main: ReactNode
  if (location.view === 'unreads') {
    main = <UnreadsView />
  } else if (location.view === 'threads') {
    main = <ThreadsView />
  } else if (loadFailed(conversations)) {
    main = (
      <Pane>
        <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-[13px] text-muted-foreground" role="alert">
          Could not load chat.
          <Button type="button" variant="outline" size="sm" onClick={() => void conversations.refetch()}>
            Try again
          </Button>
        </div>
      </Pane>
    )
  } else if (conversations.data === undefined) {
    // a blank canvas while chat loads: no "Loading…" between the app and the conversation
    main = <Pane />
  } else if (location.view === 'home') {
    main = (
      <Pane>
        <div className="flex flex-1 flex-col p-6">
          <EmptyState icon={Message} title="No conversations yet" description="Join a channel to start reading and writing messages." action={browseAction} />
        </div>
      </Pane>
    )
  } else if (!conversation) {
    main = (
      <Pane>
        <div className="flex flex-1 flex-col p-6">
          <EmptyState
            icon={Lock}
            title="This conversation is not available"
            description="It is private or it does not exist. You can find every public channel in Browse channels."
            action={browseAction}
          />
        </div>
      </Pane>
    )
  } else if (location.view === 'thread') {
    main = (
      <ThreadView
        key={location.rootId}
        conversationId={conversation.id}
        rootId={location.rootId}
        variant="full"
        focusMessageId={location.m ?? undefined}
        canResize={fits}
      />
    )
  } else if (pane?.kind === 'thread' && !fits) {
    // a thread that cannot sit beside the column takes the column; the URL stays the pane URL
    main = (
      <ThreadView
        key={pane.rootId}
        conversationId={conversation.id}
        rootId={pane.rootId}
        variant="full"
        focusMessageId={pane.focusMessageId ?? undefined}
        canResize={false}
      />
    )
  } else {
    main = (
      <Pane>
        <ConversationHeader conversation={conversation} />
        {/* with a thread pane open, `?m=` is a reply in that thread */}
        <ConversationView key={conversation.id} conversationId={conversation.id} focusMessageId={pane?.kind === 'thread' ? undefined : (location.m ?? undefined)} />
      </Pane>
    )
  }

  return (
    <ChatHostContext value={host}>
      <div
        data-slot="chat-page"
        data-view={location.view === 'home' ? 'index' : 'detail'}
        className="group/chat flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background text-foreground max-[899px]:-mt-[env(safe-area-inset-top,0px)] dark:[--background:var(--card)]"
        style={chatThemeVariables(theme)}
      >
        <ChatSidebar />
        <div ref={areaRef} data-slot="chat-area" className="flex min-h-0 min-w-0 flex-1 max-[899px]:pt-[env(safe-area-inset-top,0px)] max-[899px]:group-data-[view=index]/chat:hidden">
          <main data-slot="chat-main" className="flex min-h-0 min-w-0 flex-1 flex-col">
            {main}
          </main>
          {inlinePane && conversation ? (
            <InlinePane width={layout.width} onResize={setPaneWidth}>
              <PaneContent conversationId={conversation.id} pane={inlinePane} />
            </InlinePane>
          ) : null}
        </div>
        <PaneSheet conversationId={conversation?.id ?? null} pane={sheetPane} onClose={navigation.closePane} />
        <BrowseChannelsDialog open={browseOpen} onOpenChange={setBrowseOpen} />
        <ChatCommands />
      </div>
    </ChatHostContext>
  )
}
