// Port of the chat reference ChannelSidebar: resizable width, collapsible categories, context menus,
// and hand-rolled mouse drag-and-drop reorder for categories and channels (no DnD library).
import { useEffect, useState, type MouseEvent as ReactMouseEvent } from 'react'
import { useNavigate } from 'react-router'
import { ChevronDown, FolderAdd as FolderPlus, Hashtag as Hash, Edit as Pencil, Add as Plus, Setting2 as Settings, Trash as Trash2 } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { ContextMenu, ContextMenuContent, ContextMenuTrigger } from '@/components/ui/context-menu'
import { DialogFooter, DialogClose } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Modal } from '@/components/common/Modal'
import { PaneTitle } from '@/components/common/Pane'
import { deleteChatMessage, followThread, markChannelRead, renameThread, reorderChannels, reorderChatCategories } from '@/mock/actions'
import { Emoji } from '@/components/common/Emoji'
import { FollowIcon } from '@/components/common/icons/FollowIcon'
import { threadTitleOf } from '@/lib/messagePreview'
import type { AppState, Channel, ChatCategory, ChatMessage } from '@/mock/types'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import { ChannelModals, type ChannelModalState } from './ChannelModals'
import { ChatContextMenuItem, ChatDropdownMenuItem } from './ChatMenuItem'
import { RowButton } from './RowButton'

const WIDTH_KEY = 'orbit:channel_sidebar_width'
const COLLAPSE_KEY = 'orbit:category_collapsed'
const MIN_WIDTH = 220
const MAX_WIDTH = 420
const DEFAULT_WIDTH = 240

function clampWidth(width: number) {
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, width))
}

function storedWidth(): number {
  const value = Number(window.localStorage.getItem(WIDTH_KEY))
  return Number.isFinite(value) && value > 0 ? clampWidth(value) : DEFAULT_WIDTH
}

function storedCollapsed(): Set<string> {
  try {
    const value = JSON.parse(window.localStorage.getItem(COLLAPSE_KEY) ?? '[]')
    return Array.isArray(value) ? new Set(value.filter((v): v is string => typeof v === 'string')) : new Set()
  } catch {
    return new Set()
  }
}

/* ---------- the chat reference drag helpers ---------- */

type DropPosition = 'before' | 'after'
type ChannelDropIndicator = { channelId: string; categoryId: string; position: DropPosition }
type ChannelDragState = { channelId: string; categoryId: string }
type CategoryDropIndicator = { categoryId: string; position: DropPosition }

function getDropPosition(element: HTMLElement, clientY: number): DropPosition {
  const rect = element.getBoundingClientRect()
  return clientY < rect.top + rect.height / 2 ? 'before' : 'after'
}

function getChannelDropTarget(clientX: number, clientY: number): ChannelDropIndicator | null {
  const element = document.elementFromPoint(clientX, clientY)?.closest<HTMLElement>('[data-channel-drop-id]')
  const channelId = element?.getAttribute('data-channel-drop-id')
  const categoryId = element?.getAttribute('data-channel-category-id')
  if (!element || !channelId || !categoryId) return null
  return { channelId, categoryId, position: getDropPosition(element, clientY) }
}

function getCategoryDropTarget(clientX: number, clientY: number): CategoryDropIndicator | null {
  const element = document.elementFromPoint(clientX, clientY)?.closest<HTMLElement>('[data-category-drop-id]')
  const categoryId = element?.getAttribute('data-category-drop-id')
  if (!element || !categoryId) return null
  return { categoryId, position: getDropPosition(element, clientY) }
}

function moveItem<T extends { id: string }>(items: T[], draggedId: string, targetId: string, position: DropPosition): T[] {
  const next = [...items]
  const fromIndex = next.findIndex((item) => item.id === draggedId)
  const targetIndex = next.findIndex((item) => item.id === targetId)
  if (fromIndex === -1 || targetIndex === -1) return next
  const [dragged] = next.splice(fromIndex, 1)
  const adjustedTargetIndex = next.findIndex((item) => item.id === targetId)
  if (adjustedTargetIndex === -1) return items
  next.splice(position === 'after' ? adjustedTargetIndex + 1 : adjustedTargetIndex, 0, dragged)
  return next
}

function DropLine({ position }: { position: DropPosition | null }) {
  if (!position) return null
  return (
    <span
      aria-hidden="true"
      className="pointer-events-none absolute right-0 left-0 z-10 h-0.5 rounded-full bg-primary shadow-[0_0_0_1px_rgba(0,0,0,0.35)] data-[position=before]:top-0 data-[position=after]:bottom-0"
      data-position={position}
    />
  )
}

export function ChannelSidebar({
  state,
  activeChannelId,
  activeThreadId = null,
  onOpenThread,
}: {
  state: AppState
  activeChannelId: string | null
  /** Open thread root id: the thread row is active and the channel row is not. */
  activeThreadId?: string | null
  onOpenThread?: (channelId: string, rootId: string) => void
}) {
  const navigate = useNavigate()
  const [width, setWidth] = useState(storedWidth)
  const [collapsed, setCollapsed] = useState<Set<string>>(storedCollapsed)
  const [modal, setModal] = useState<ChannelModalState>(null)
  const [deleteThread, setDeleteThread] = useState<ChatMessage | null>(null)
  const [renamingThread, setRenamingThread] = useState<ChatMessage | null>(null)
  const [threadNameDraft, setThreadNameDraft] = useState('')

  const [dragChannel, setDragChannel] = useState<ChannelDragState | null>(null)
  const [dropIndicator, setDropIndicator] = useState<ChannelDropIndicator | null>(null)
  const [dragCategoryId, setDragCategoryId] = useState<string | null>(null)
  const [categoryDropIndicator, setCategoryDropIndicator] = useState<CategoryDropIndicator | null>(null)

  useEffect(() => {
    window.localStorage.setItem(WIDTH_KEY, String(width))
  }, [width])

  useEffect(() => {
    window.localStorage.setItem(COLLAPSE_KEY, JSON.stringify([...collapsed]))
  }, [collapsed])

  function handleResizeStart(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault()
    const startX = event.clientX
    const startWidth = width
    function onMove(moveEvent: PointerEvent) {
      setWidth(clampWidth(startWidth + (moveEvent.clientX - startX)))
    }
    function onUp() {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  function toggleCategory(catId: string) {
    setCollapsed((prev) => {
      const next = new Set(prev)
      if (next.has(catId)) next.delete(catId)
      else next.add(catId)
      return next
    })
  }

  function selectChannel(channelId: string) {
    markChannelRead(channelId)
    navigate(`/chat/${channelId}`)
  }

  /** Followed threads only: non-reply roots with replies or a thread start, oldest first. */
  function threadsForChannel(channelId: string): ChatMessage[] {
    return state.chatMessages
      .filter(
        (m) =>
          m.channelId === channelId &&
          !m.threadRootId &&
          m.threadFollowed &&
          (m.startsThread || state.chatMessages.some((r) => r.threadRootId === m.id)),
      )
      .sort((a, b) => a.createdAt.localeCompare(b.createdAt))
  }

  function commitThreadRename() {
    if (!renamingThread) return
    const name = threadNameDraft.trim()
    if (!name) return
    renameThread(renamingThread.id, name)
    setRenamingThread(null)
    setThreadNameDraft('')
  }

  /* ---- channel drag (the chat reference: channels cannot move between categories) ---- */

  function startChannelDrag(channelId: string, categoryId: string) {
    setDragChannel({ channelId, categoryId })
    setDropIndicator(null)
    setDragCategoryId(null)
    setCategoryDropIndicator(null)

    function handleMouseUp(event: MouseEvent) {
      const target = getChannelDropTarget(event.clientX, event.clientY)
      if (target && target.channelId !== channelId && target.categoryId === categoryId) {
        const inCategory = state.channels.filter((c) => c.categoryId === categoryId)
        const next = moveItem(inCategory, channelId, target.channelId, target.position)
        reorderChannels(categoryId, next.map((c) => c.id))
      }
      setDragChannel(null)
      setDropIndicator(null)
    }

    document.addEventListener('mouseup', handleMouseUp, { once: true })
  }

  function handleChannelDragOver(channelId: string, categoryId: string, element: HTMLElement, clientY: number) {
    if (!dragChannel || dragChannel.channelId === channelId || dragChannel.categoryId !== categoryId) {
      setDropIndicator(null)
      return
    }
    const position = getDropPosition(element, clientY)
    setDropIndicator((current) =>
      current?.channelId === channelId && current.position === position ? current : { channelId, categoryId, position },
    )
  }

  function handleChannelDragLeave(channelId: string) {
    setDropIndicator((current) => (current?.channelId === channelId ? null : current))
  }

  /* ---- category drag ---- */

  function startCategoryDrag(categoryId: string) {
    setDragCategoryId(categoryId)
    setCategoryDropIndicator(null)
    setDragChannel(null)
    setDropIndicator(null)

    function handleMouseUp(event: MouseEvent) {
      const target = getCategoryDropTarget(event.clientX, event.clientY)
      if (target && target.categoryId !== categoryId) {
        const next = moveItem(state.chatCategories, categoryId, target.categoryId, target.position)
        reorderChatCategories(next.map((c) => c.id))
      }
      setDragCategoryId(null)
      setCategoryDropIndicator(null)
    }

    document.addEventListener('mouseup', handleMouseUp, { once: true })
  }

  function handleCategoryDragOver(categoryId: string, element: HTMLElement, clientY: number) {
    if (!dragCategoryId || dragCategoryId === categoryId) {
      setCategoryDropIndicator(null)
      return
    }
    const position = getDropPosition(element, clientY)
    setCategoryDropIndicator((current) =>
      current?.categoryId === categoryId && current.position === position ? current : { categoryId, position },
    )
  }

  function handleCategoryDragLeave(categoryId: string) {
    setCategoryDropIndicator((current) => (current?.categoryId === categoryId ? null : current))
  }

  function threadMenuItems(thread: ChatMessage) {
    return (
      <>
        <ChatContextMenuItem onClick={() => followThread(thread.id, !thread.threadFollowed)}>
          <FollowIcon size={16} />
          {thread.threadFollowed ? 'Unfollow Thread' : 'Follow Thread'}
        </ChatContextMenuItem>
        <ChatContextMenuItem
          onClick={() => {
            setRenamingThread(thread)
            setThreadNameDraft(threadTitleOf(thread))
          }}
        >
          <Pencil size={16} />
          Rename Thread
        </ChatContextMenuItem>
        <ChatContextMenuItem danger onClick={() => setDeleteThread(thread)}>
          <Trash2 size={16} />
          Delete Thread
        </ChatContextMenuItem>
      </>
    )
  }

  function channelMenuItems(channel: Channel) {
    return (
      <>
        <ChatContextMenuItem onClick={() => setModal({ kind: 'edit-channel', channel })}>
          <Pencil size={16} />
          Edit Channel
        </ChatContextMenuItem>
        <ChatContextMenuItem danger onClick={() => setModal({ kind: 'delete-channel', channel })}>
          <Trash2 size={16} />
          Delete Channel
        </ChatContextMenuItem>
      </>
    )
  }

  function categoryMenuItems(category: ChatCategory) {
    return (
      <>
        <ChatContextMenuItem onClick={() => setModal({ kind: 'edit-category', category })}>
          <Pencil size={16} />
          Edit Category
        </ChatContextMenuItem>
        <ChatContextMenuItem
          onClick={() => setModal({ kind: 'create-channel', categoryId: category.id, categoryName: category.name })}
        >
          <Plus size={16} />
          Create Channel
        </ChatContextMenuItem>
        <ChatContextMenuItem
          danger
          onClick={() => setModal({ kind: 'delete-category', id: category.id, name: category.name })}
        >
          <Trash2 size={16} />
          Delete Category
        </ChatContextMenuItem>
      </>
    )
  }

  return (
    <div className="relative flex shrink-0 flex-col border-r border-border bg-sidebar text-sidebar-foreground max-[899px]:w-full! max-[899px]:border-r-0 max-[899px]:group-data-[view=conversation]/chat:hidden" style={{ width }}>
      <div className="absolute top-0 -right-px bottom-0 z-20 w-1 cursor-col-resize transition-colors hover:bg-primary/40 max-[899px]:hidden" onPointerDown={handleResizeStart} title="Resize channel sidebar" />

      {/* server header + dropdown */}
      <DropdownMenu>
        <DropdownMenuTrigger render={<RowButton className="relative h-12 shrink-0 justify-between gap-1 border-b border-border px-3 font-semibold text-foreground/80 hover:text-foreground" />}>
          <PaneTitle>Chat</PaneTitle>
          <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
        </DropdownMenuTrigger>
        <DropdownMenuContent sideOffset={0} alignOffset={8} className="w-[calc(var(--anchor-width)-1rem)]">
          <ChatDropdownMenuItem
            onClick={() => {
              const first = state.chatCategories[0]
              if (first) setModal({ kind: 'create-channel', categoryId: first.id, categoryName: first.name })
            }}
          >
            <Plus size={16} />
            Create channel
          </ChatDropdownMenuItem>
          <ChatDropdownMenuItem onClick={() => setModal({ kind: 'create-category' })}>
            <FolderPlus size={16} />
            Create category
          </ChatDropdownMenuItem>
          <DropdownMenuSeparator />
          <ChatDropdownMenuItem onClick={() => navigate('/chat/settings')}>
            <Settings size={16} />
            Chat Settings
          </ChatDropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      {/* channel list */}
      <div className="flex min-h-0 flex-1 flex-col overflow-auto [overscroll-behavior:none]">
        <div className="flex w-full min-w-0 flex-col gap-1 pt-2">
          {state.chatCategories.map((cat) => {
            const isCollapsed = collapsed.has(cat.id)
            const channels = state.channels.filter((c) => c.categoryId === cat.id)
            return (
              <div key={cat.id} className="flex min-w-0 flex-col px-2 pt-5 first:pt-3">
                <ContextMenu>
                  <ContextMenuTrigger render={<div className="group flex w-full items-center px-1 pb-1" />}>
                    <RowButton
                      className="group/toggle relative flex-1 cursor-pointer gap-1 text-xs leading-5 font-bold text-muted-foreground hover:text-foreground data-[dragging=true]:text-foreground"
                      data-category-drop-id={cat.id}
                      data-collapsed={isCollapsed ? 'true' : undefined}
                      data-dragging={dragCategoryId === cat.id ? 'true' : undefined}
                      onClick={() => toggleCategory(cat.id)}
                      onMouseMove={(event: ReactMouseEvent<HTMLButtonElement>) =>
                        handleCategoryDragOver(cat.id, event.currentTarget, event.clientY)
                      }
                      onMouseLeave={() => handleCategoryDragLeave(cat.id)}
                      onMouseDown={(event) => {
                        if (event.button !== 0) return
                        startCategoryDrag(cat.id)
                      }}
                    >
                      <DropLine
                        position={categoryDropIndicator?.categoryId === cat.id ? categoryDropIndicator.position : null}
                      />
                      <span className="flex min-w-0 items-center gap-1.5">
                        {cat.emoji ? <span className="inline-flex size-3.5 shrink-0 items-center justify-center text-[10.5px] leading-none"><Emoji value={cat.emoji} size={13} /></span> : null}
                        <span className="truncate">{cat.name}</span>
                      </span>
                      <ChevronDown className="ml-0.5 size-2 shrink-0 transition-transform group-data-[collapsed=true]/toggle:-rotate-90" />
                    </RowButton>
                    <RowButton
                      className="w-auto justify-center text-muted-foreground transition-[opacity,color] duration-150 hover:text-foreground focus-visible:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100"
                      title="Create Channel"
                      aria-label="Create Channel"
                      onClick={() => setModal({ kind: 'create-channel', categoryId: cat.id, categoryName: cat.name })}
                    >
                      <Plus className="size-2.5" />
                    </RowButton>
                  </ContextMenuTrigger>
                  <ContextMenuContent className="min-w-44">{categoryMenuItems(cat)}</ContextMenuContent>
                </ContextMenu>
                {/* stays mounted so collapse can animate grid rows 1fr -> 0fr; inert keeps hidden rows out of focus */}
                <div
                  className="grid grid-rows-[1fr] transition-[grid-template-rows] duration-200 ease-out data-[collapsed=true]:grid-rows-[0fr] motion-reduce:transition-none"
                  data-collapsed={isCollapsed ? 'true' : undefined}
                >
                  <div className="flex min-h-0 flex-col gap-0.5 overflow-hidden" inert={isCollapsed}>
                    {channels.map((ch) => {
                      const isActive = ch.id === activeChannelId && !activeThreadId
                      const threads = threadsForChannel(ch.id)
                      return (
                        <div key={ch.id} className="min-w-0">
                        <ContextMenu>
                          <ContextMenuTrigger
                            render={
                              <RowButton
                                className="relative cursor-pointer rounded-md px-2 py-1.5 text-sm leading-5 font-medium text-muted-foreground hover:bg-sidebar-accent/50 hover:text-foreground focus-visible:ring-inset data-[unread=true]:font-semibold data-[unread=true]:text-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-sidebar-accent-foreground data-[dragging=true]:bg-sidebar-accent data-[dragging=true]:text-foreground [&>svg]:size-4"
                                data-channel-drop-id={ch.id}
                                data-channel-category-id={cat.id}
                                data-active={isActive ? 'true' : undefined}
                                data-unread={ch.unreadCount > 0 ? 'true' : undefined}
                                data-dragging={dragChannel?.channelId === ch.id ? 'true' : undefined}
                                onClick={() => selectChannel(ch.id)}
                                onMouseMove={(event: ReactMouseEvent<HTMLButtonElement>) =>
                                  handleChannelDragOver(ch.id, cat.id, event.currentTarget, event.clientY)
                                }
                                onMouseLeave={() => handleChannelDragLeave(ch.id)}
                                onMouseDown={(event) => {
                                  if (event.button !== 0) return
                                  startChannelDrag(ch.id, cat.id)
                                }}
                              />
                            }
                          >
                            <DropLine position={dropIndicator?.channelId === ch.id ? dropIndicator.position : null} />
                            {ch.emoji ? <span className="inline-flex size-4 shrink-0 items-center justify-center text-xs leading-none"><Emoji value={ch.emoji} size={16} /></span> : <Hash size={16} />}
                            <span className="min-w-0 truncate">{ch.name}</span>
                            {ch.unreadCount > 0 && !isActive ? (
                              <span className="ml-auto flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-[10px] font-semibold text-primary-foreground tabular-nums">{ch.unreadCount}</span>
                            ) : null}
                          </ContextMenuTrigger>
                          <ContextMenuContent className="min-w-44">{channelMenuItems(ch)}</ContextMenuContent>
                        </ContextMenu>
                        {threads.length > 0 ? (
                          <div className="ml-3.5 py-0.5">
                            {threads.map((thread, index) => {
                              const isThreadActive = activeChannelId === ch.id && activeThreadId === thread.id
                              return (
                                <ContextMenu key={thread.id}>
                                  <ContextMenuTrigger
                                    render={
                                      <RowButton
                                        className="group/row relative gap-0 rounded-md py-1 pr-2 pl-6 text-xs leading-5 font-semibold text-muted-foreground hover:text-foreground focus-visible:ring-inset data-[active=true]:text-foreground"
                                        data-active={isThreadActive ? 'true' : undefined}
                                        onClick={() => onOpenThread?.(ch.id, thread.id)}
                                      />
                                    }
                                  >
                                    <span aria-hidden className="pointer-events-none absolute top-0 bottom-0 left-0 z-10 w-2.5 rounded-bl-md border-b-2 border-l-2 border-border/80 data-[last=true]:bottom-auto data-[last=true]:h-3" data-last={index === threads.length - 1 ? 'true' : undefined} />
                                    <span aria-hidden className="pointer-events-none absolute inset-0 left-3.5 rounded-md transition-colors group-hover/row:bg-sidebar-accent/45 group-data-[active=true]/row:bg-sidebar-accent" />
                                    <span className="relative z-10 min-w-0 truncate">{threadTitleOf(thread)}</span>
                                  </ContextMenuTrigger>
                                  <ContextMenuContent className="min-w-44">{threadMenuItems(thread)}</ContextMenuContent>
                                </ContextMenu>
                              )
                            })}
                          </div>
                        ) : null}
                        </div>
                      )
                    })}
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      </div>

      <ChannelModals modal={modal} onClose={() => setModal(null)} activeChannelId={activeChannelId} />
      {deleteThread ? (
        <ConfirmDeleteModal
          title="Delete thread?"
          description={`This will permanently delete the thread "${threadTitleOf(deleteThread)}".`}
          onClose={() => setDeleteThread(null)}
          onConfirm={() => {
            deleteChatMessage(deleteThread.id)
            setDeleteThread(null)
          }}
        />
      ) : null}
      {renamingThread ? (
        <Modal title="Rename thread" onClose={() => setRenamingThread(null)} className="sm:max-w-sm">
          <Field>
            <FieldLabel htmlFor="rename-thread-name">Thread name</FieldLabel>
            <Input
              id="rename-thread-name"
              value={threadNameDraft}
              autoFocus
              onChange={(e) => setThreadNameDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitThreadRename()
              }}
            />
          </Field>
          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" />}>
              Cancel
            </DialogClose>
            <Button type="button" disabled={!threadNameDraft.trim()} onClick={commitThreadRename}>
              Save
            </Button>
          </DialogFooter>
        </Modal>
      ) : null}
    </div>
  )
}
