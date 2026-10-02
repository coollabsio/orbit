import { useState, type ComponentType, type PointerEvent } from 'react'
import { toast } from 'sonner'
import { Add, Brush, ChevronDown, ChevronRight, DirectInbox, Edit, MoreH, Trash } from 'reicon-react'
import { ThreadIcon } from '@/components/common/icons/ThreadIcon'
import { confirmAction } from '@/components/common/confirmAction'
import { PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useDeleteCategory, usePlaceCategory, usePlaceChannel } from '@/features/chat/api/mutations'
import { useCategories, useChatBadges, useConversations, useConversationStates } from '@/features/chat/api/queries'
import type { Category } from '@/features/chat/api/types'
import { THREADS_PATH, UNREADS_PATH } from '@/features/chat/chatRoutes'
import { BrowseChannelsDialog } from '@/features/chat/components/dialogs/BrowseChannelsDialog'
import { CategoryDialog } from '@/features/chat/components/dialogs/CategoryDialog'
import { EditChannelDialog } from '@/features/chat/components/dialogs/EditChannelDialog'
import { NewChannelDialog } from '@/features/chat/components/dialogs/NewChannelDialog'
import { NewMessageDialog } from '@/features/chat/components/dialogs/NewMessageDialog'
import { chatSidebarVariables, useChatTheme } from '@/features/chat/lib/chatTheme'
import { buildSidebarSections, conversationBadge, dropBefore, type SidebarSection } from '@/features/chat/lib/sidebar'
import { useChatLocation } from '@/features/chat/useChatNavigation'
import { useMembers } from '@/features/workspaces/api'
import { ChatRow, ChatRowEnd, ChatRowLink, ChatRowName, CountBadge } from './ChatRow'
import { ChatThemeDialog } from './ChatThemeDialog'
import { ConversationRow } from './ConversationRow'
import { ResizeHandle } from './ResizeHandle'
import { visibleRows } from './sidebarRows'
import { useStoredWidth, type WidthRange } from './storedWidth'
import { useIsChatAdmin } from './useConversationActions'
import { useSidebarDrag, type SidebarDrag, type SidebarDrop } from './useSidebarDrag'

const SIDEBAR_WIDTH: WidthRange = { min: 220, max: 420, initial: 260 }

type SidebarDialog =
  | { kind: 'message' }
  | { kind: 'channel'; categoryId: string | null }
  | { kind: 'browse' }
  | { kind: 'theme' }
  | { kind: 'category'; category?: Category }
  | { kind: 'edit'; conversationId: string }

/** The sections the user collapsed, remembered for each workspace. */
function useCollapsedSections(workspaceId: string): [ReadonlySet<string>, (key: string) => void] {
  const storageKey = `orbit:chat:collapsed:${workspaceId}`
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => {
    try {
      const parsed: unknown = JSON.parse(window.localStorage.getItem(storageKey) ?? '[]')
      return new Set(Array.isArray(parsed) ? parsed.filter((key): key is string => typeof key === 'string') : [])
    } catch {
      return new Set()
    }
  })
  const toggle = (key: string) => {
    const next = new Set(collapsed)
    if (!next.delete(key)) next.add(key)
    setCollapsed(next)
    window.localStorage.setItem(storageKey, JSON.stringify([...next]))
  }
  return [collapsed, toggle]
}

/** `Unreads` and `Threads`: pinned above the scrolling list, each with its count. */
function PinnedRow({ to, icon: Icon, label, count, active }: { to: string; icon: ComponentType<{ size?: number; className?: string }>; label: string; count: number; active: boolean }) {
  return (
    <ChatRow data-active={active} data-unread={count > 0}>
      <ChatRowLink to={to} className="pr-2" aria-current={active ? 'page' : undefined} aria-label={count > 0 ? `${label}, ${count} unread` : label}>
        <span className="flex size-[18px] shrink-0 items-center justify-center text-muted-foreground">
          <Icon size={16} />
        </span>
        <ChatRowName>{label}</ChatRowName>
      </ChatRowLink>
      {count > 0 ? (
        <ChatRowEnd>
          <CountBadge count={count} />
        </ChatRowEnd>
      ) : null}
    </ChatRow>
  )
}

/**
 * A section label: sentence case, 12px, muted. It collapses the section; its actions show on hover and focus. A
 * category's label is the handle to drag the category (`onDragStart`); `dropInto` marks the section that a dragged
 * channel would go to the end of.
 */
function SectionHeader({
  section,
  collapsed,
  admin,
  dropInto,
  onDragStart,
  onToggle,
  onAdd,
  onRename,
}: {
  section: SidebarSection
  collapsed: boolean
  admin: boolean
  dropInto: boolean
  onDragStart: ((event: PointerEvent) => void) | undefined
  onToggle: () => void
  /** New channel in this category, or a new message for the DM section. */
  onAdd: (() => void) | null
  onRename: (category: Category) => void
}) {
  const deleteCategory = useDeleteCategory()
  const category = admin ? section.category : null
  const Chevron = collapsed ? ChevronRight : ChevronDown

  const remove = async (target: Category) => {
    const confirmed = await confirmAction({
      title: `Delete the category “${target.name}”?`,
      description: 'Its channels are kept and move to “Channels”.',
      confirmLabel: 'Delete category',
      danger: true,
    })
    if (confirmed) deleteCategory.mutate(target.id, { onError: () => void toast.error('Could not delete the category. Try again.') })
  }

  return (
    <div
      data-slot="chat-section-header"
      data-drop={dropInto || undefined}
      className="group/row flex h-7 shrink-0 items-center gap-0.5 rounded-md pr-1.5 data-[drop]:bg-primary/10 data-[drop]:ring-1 data-[drop]:ring-primary/25 data-[drop]:ring-inset"
    >
      <Button
        type="button"
        variant="ghost"
        size="xs"
        className="min-w-0 flex-1 justify-start gap-1 px-2 font-normal text-muted-foreground transition-none select-none aria-expanded:bg-transparent aria-expanded:text-muted-foreground hover:aria-expanded:bg-muted"
        aria-expanded={!collapsed}
        onClick={onToggle}
        onPointerDown={onDragStart}
      >
        <Chevron className="size-3" aria-hidden="true" />
        <span className="truncate">{section.title}</span>
      </Button>
      <span className="flex shrink-0 items-center gap-0.5 hover-fine:opacity-0 hover-fine:group-focus-within/row:opacity-100 hover-fine:group-hover/row:opacity-100 hover-fine:has-[[aria-expanded=true]]:opacity-100">
        {category ? (
          <DropdownMenu>
            <DropdownMenuTrigger
              render={<Button type="button" variant="ghost" size="icon-xs" className="text-muted-foreground transition-none" aria-label={`Options for the category ${category.name}`} />}
            >
              <MoreH className="size-4" />
            </DropdownMenuTrigger>
            <DropdownMenuContent className="w-auto min-w-44">
              <DropdownMenuItem onClick={() => onRename(category)}>
                <Edit />
                Rename
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem variant="destructive" onClick={() => void remove(category)}>
                <Trash />
                Delete category
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
        {onAdd ? (
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="text-muted-foreground transition-none"
            aria-label={section.kind === 'dms' ? 'New message' : `New channel in ${section.title}`}
            onClick={onAdd}
          >
            <Add className="size-4" />
          </Button>
        ) : null}
      </span>
    </div>
  )
}

/**
 * The chat sidebar: one list. `Unreads` and `Threads` are pinned; below them Favorites, the shared categories,
 * "Channels" and Direct messages scroll. On a phone it is the whole `/chat` screen (the parent `group/chat` hides it
 * on the other chat screens).
 */
export function ChatSidebar() {
  const location = useChatLocation()
  const { workspaceId, currentUserId } = useChatContext()
  const conversations = useConversations()
  const categories = useCategories().data
  const states = useConversationStates().data
  const people = useMembers(workspaceId).data ?? []
  const badges = useChatBadges()
  const admin = useIsChatAdmin()
  const [width, setWidth] = useStoredWidth('orbit:chat:sidebar_width', SIDEBAR_WIDTH)
  const [collapsed, toggleCollapsed] = useCollapsedSections(workspaceId)
  const [showAllDms, setShowAllDms] = useState(false)
  const theme = useChatTheme()
  // the last dialog stays set while it closes, so it can play its exit
  const [dialog, setDialog] = useState<SidebarDialog | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const show = (next: SidebarDialog) => {
    setDialog(next)
    setDialogOpen(true)
  }
  const isOpen = (kind: SidebarDialog['kind']) => dialogOpen && dialog?.kind === kind

  const activeId = location.view === 'conversation' || location.view === 'thread' ? location.conversationId : null
  const stateById = new Map((states ?? []).map((state) => [state.conversationId, state]))
  const byId = new Map((conversations.data ?? []).map((conversation) => [conversation.id, conversation]))
  const isUnread = (id: string) => {
    const conversation = byId.get(id)
    if (!conversation) return false
    const badge = conversationBadge(conversation, stateById.get(id))
    return badge.bold || badge.count > 0
  }
  const built = conversations.data && currentUserId ? buildSidebarSections(conversations.data, categories ?? [], states ?? []) : []

  // Channel order and categories are shared, so only a chat admin drags them.
  const placeChannel = usePlaceChannel()
  const placeCategory = usePlaceCategory()
  const onDrop = (dragged: SidebarDrag, target: SidebarDrop) => {
    if (dragged.kind === 'category') {
      if (target.kind !== 'category') return
      const order = built.flatMap((section) => (section.category ? [section.category] : []))
      placeCategory.mutate(
        { categoryId: dragged.id, beforeId: dropBefore(order, dragged.id, target.id, target.zone) },
        { onError: () => void toast.error('Could not move the category. Try again.') },
      )
      return
    }
    const section =
      target.kind === 'section'
        ? built.find((candidate) => candidate.key === target.key)
        : target.kind === 'row'
          ? built.find((candidate) => candidate.conversations.some((conversation) => conversation.id === target.id))
          : undefined
    // "Channels" shows only during the drag when it is empty, so it is not in `built`: it is the place without a category.
    if (!section && !(target.kind === 'section' && target.key === 'channels')) return
    placeChannel.mutate(
      {
        conversationId: dragged.id,
        categoryId: section?.category?.id ?? null,
        beforeId: section && target.kind === 'row' ? dropBefore(section.conversations, dragged.id, target.id, target.zone) : null,
      },
      { onError: () => void toast.error('Could not move the channel. Try again.') },
    )
  }
  const { drag, drop, start } = useSidebarDrag(onDrop)
  // While a channel is dragged, "Channels" shows also when it is empty: it is where a channel leaves its category.
  const sections =
    drag?.kind === 'channel' && !built.some((section) => section.kind === 'channels')
      ? built.flatMap((section): SidebarSection[] =>
          section.kind === 'dms' ? [{ key: 'channels', kind: 'channels', title: 'Channels', category: null, conversations: [] }, section] : [section],
        )
      : built

  return (
    <aside
      data-slot="chat-sidebar"
      aria-label="Chat"
      className="relative flex min-h-0 shrink-0 flex-col border-r bg-background max-[899px]:w-full! max-[899px]:border-r-0 max-[899px]:group-data-[view=detail]/chat:hidden"
      data-themed={theme ? '' : undefined}
      style={{ width, ...chatSidebarVariables(theme) }}
    >
      <PaneHeader className="gap-1 pr-2">
        <PaneTitle render={<h1 />} className="flex-1">
          Chat
        </PaneTitle>
        <Button type="button" variant="ghost" size="icon" className="text-muted-foreground" aria-label="Chat theme" onClick={() => show({ kind: 'theme' })}>
          <Brush size={20} weight="Filled" />
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon" className="text-muted-foreground" aria-label="New" />}>
            <Add size={20} weight="Filled" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-auto min-w-44">
            <DropdownMenuItem onClick={() => show({ kind: 'message' })}>New message</DropdownMenuItem>
            <DropdownMenuItem onClick={() => show({ kind: 'channel', categoryId: null })}>New channel</DropdownMenuItem>
            <DropdownMenuItem onClick={() => show({ kind: 'browse' })}>Browse channels</DropdownMenuItem>
            {admin ? (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={() => show({ kind: 'category' })}>New category</DropdownMenuItem>
              </>
            ) : null}
          </DropdownMenuContent>
        </DropdownMenu>
      </PaneHeader>

      <div className="flex shrink-0 flex-col gap-px px-2 pt-2">
        <PinnedRow to={UNREADS_PATH} icon={DirectInbox} label="Unreads" count={badges.unreads} active={location.view === 'unreads'} />
        <PinnedRow to={THREADS_PATH} icon={ThreadIcon} label="Threads" count={badges.threads} active={location.view === 'threads'} />
      </div>

      <nav aria-label="Conversations" className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-2 pt-3">
        {conversations.isError ? (
          <div className="flex flex-col items-start gap-2 px-2 text-[13px] text-muted-foreground" role="alert">
            Could not load your conversations.
            <Button type="button" variant="outline" size="sm" onClick={() => void conversations.refetch()}>
              Try again
            </Button>
          </div>
        ) : null}
        {sections.map((section) => {
          const sectionCollapsed = collapsed.has(section.key)
          const { rows, hidden } = visibleRows(section, { collapsed: sectionCollapsed, showAllDms, activeId, isUnread })
          const onAdd =
            section.kind === 'dms'
              ? () => show({ kind: 'message' })
              : section.kind === 'favorites'
                ? null
                : () => show({ kind: 'channel', categoryId: section.category?.id ?? null })
          // Favorites and direct messages have their own order.
          const sortable = admin && (section.kind === 'category' || section.kind === 'channels')
          const category = sortable ? section.category : null
          return (
            <section
              key={section.key}
              aria-label={section.title}
              data-drop-section={sortable ? section.key : undefined}
              data-drop-category={category?.id}
              data-dragging={(drag?.kind === 'category' && drag.id === category?.id) || undefined}
              data-drop={drop?.kind === 'category' && drop.id === category?.id ? drop.zone : undefined}
              className="relative flex flex-col gap-px data-[dragging]:opacity-40 data-[drop=after]:after:absolute data-[drop=after]:after:inset-x-1.5 data-[drop=after]:after:-bottom-1.5 data-[drop=after]:after:h-0.5 data-[drop=after]:after:rounded-[1px] data-[drop=after]:after:bg-primary data-[drop=after]:after:content-[''] data-[drop=before]:before:absolute data-[drop=before]:before:inset-x-1.5 data-[drop=before]:before:-top-1.5 data-[drop=before]:before:h-0.5 data-[drop=before]:before:rounded-[1px] data-[drop=before]:before:bg-primary data-[drop=before]:before:content-['']"
            >
              <SectionHeader
                section={section}
                collapsed={sectionCollapsed}
                admin={admin}
                dropInto={drop?.kind === 'section' && drop.key === section.key}
                onDragStart={category ? (event) => start(event, { kind: 'category', id: category.id }) : undefined}
                onToggle={() => toggleCollapsed(section.key)}
                onAdd={onAdd}
                onRename={(category) => show({ kind: 'category', category })}
              />
              {rows.map((conversation) => (
                <ConversationRow
                  key={conversation.id}
                  conversation={conversation}
                  people={people}
                  active={conversation.id === activeId}
                  drag={
                    sortable
                      ? {
                          dragging: drag?.kind === 'channel' && drag.id === conversation.id,
                          drop: drop?.kind === 'row' && drop.id === conversation.id ? drop.zone : undefined,
                          onPointerDown: (event) => start(event, { kind: 'channel', id: conversation.id }),
                        }
                      : undefined
                  }
                  onEdit={(conversationId) => show({ kind: 'edit', conversationId })}
                />
              ))}
              {hidden > 0 ? (
                <Button type="button" variant="ghost" size="sm" className="justify-start px-2 font-normal text-muted-foreground transition-none" onClick={() => setShowAllDms(true)}>
                  Show {hidden} more
                </Button>
              ) : null}
            </section>
          )
        })}
      </nav>

      <ResizeHandle edge="right" width={width} range={SIDEBAR_WIDTH} label="Resize the chat sidebar" onResize={setWidth} />

      <NewMessageDialog open={isOpen('message')} onOpenChange={setDialogOpen} />
      <NewChannelDialog open={isOpen('channel')} onOpenChange={setDialogOpen} defaultCategoryId={dialog?.kind === 'channel' ? dialog.categoryId : null} />
      <BrowseChannelsDialog open={isOpen('browse')} onOpenChange={setDialogOpen} />
      {dialog?.kind === 'theme' ? <ChatThemeDialog open={dialogOpen} onOpenChange={setDialogOpen} /> : null}
      {dialog?.kind === 'category' ? (
        <CategoryDialog key={dialog.category?.id ?? 'new'} open={dialogOpen} onOpenChange={setDialogOpen} category={dialog.category} />
      ) : null}
      {dialog?.kind === 'edit' ? (
        <EditChannelDialog key={dialog.conversationId} conversationId={dialog.conversationId} open={dialogOpen} onOpenChange={setDialogOpen} />
      ) : null}
    </aside>
  )
}
