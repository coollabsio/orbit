import { useRef, useState, type ReactNode } from 'react'
import { MoreH, Paperclip2, People, SearchNormal, Xmark } from 'reicon-react'
import { cn } from 'cn'
import { PinIcon } from '@/components/common/icons/PinIcon'
import { ThreadIcon } from '@/components/common/icons/ThreadIcon'
import { PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { Toggle } from '@/components/ui/toggle'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useChatContext } from '@/features/chat/api/chatContext'
import type { Conversation } from '@/features/chat/api/types'
import type { ChatPane } from '@/features/chat/chatRoutes'
import { EditChannelDialog } from '@/features/chat/components/dialogs/EditChannelDialog'
import { conversationTitle } from '@/features/chat/lib/sidebar'
import { useChatLocation, useChatNavigation } from '@/features/chat/useChatNavigation'
import { useMembers } from '@/features/workspaces/api'
import { ChatBackLink } from './sidebar/ChatRow'
import { ConversationIcon } from './sidebar/ConversationIcon'
import { ConversationMenuItems } from './sidebar/ConversationMenuItems'
import { useConversationActions } from './sidebar/useConversationActions'

/** A header button that shows a right pane and closes it again; pressed while its pane is open. */
function PaneToggle({ label, pressed, className, onToggle, children }: { label: string; pressed: boolean; className?: string; onToggle: () => void; children: ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={<Toggle aria-label={label} pressed={pressed} onPressedChange={onToggle} className={cn('size-8 min-w-8 px-0 text-muted-foreground aria-pressed:text-foreground', className)} />}
      >
        {children}
      </TooltipTrigger>
      <TooltipContent side="bottom">{label}</TooltipContent>
    </Tooltip>
  )
}

/**
 * The 48px header of the message column. Channel: icon, name, topic. DM: avatar or stacked avatars, names, presence
 * for a 1:1 (no job title). Then search, the pane toggles and the `…` menu. On a phone: Back, the name, and icons only
 * (search opens from its icon; Threads, Pins and Files are in the `…` menu).
 */
export function ConversationHeader({ conversation }: { conversation: Conversation }) {
  const location = useChatLocation()
  const navigation = useChatNavigation()
  const { workspaceId, currentUserId } = useChatContext()
  const people = useMembers(workspaceId).data ?? []
  const actions = useConversationActions(conversation)
  const [editOpen, setEditOpen] = useState(false)
  // a narrow header (a phone, or a column next to an open pane): the search field takes the place of the name
  const [searching, setSearching] = useState(false)
  const searchInput = useRef<HTMLInputElement>(null)

  const here = location.view === 'conversation' ? location : null
  const query = here?.q ?? ''
  const openPane = here && !here.thread && !here.q ? here.pane : null
  const title = conversationTitle(conversation, people, currentUserId ?? '')
  const channel = conversation.kind !== 'dm'
  const toggle = (pane: ChatPane) => () => navigation.togglePane(pane)

  return (
    <PaneHeader data-slot="conversation-header" data-searching={searching || undefined} className="group/header @container/header max-[899px]:gap-1 max-[899px]:px-2">
      <ChatBackLink className="group-data-searching/header:hidden" />
      <div className="flex min-w-0 flex-1 items-center gap-2 group-data-searching/header:hidden">
        <ConversationIcon conversation={conversation} people={people} currentUserId={currentUserId} size={20} group="stack" filled />
        <PaneTitle render={<h1 />} className="max-w-[60%] shrink-0 text-sm">
          {title}
        </PaneTitle>
        {channel && conversation.topic ? (
          <Tooltip>
            <TooltipTrigger render={<span tabIndex={0} className="min-w-0 truncate rounded-sm text-xs text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50 @max-[620px]/header:hidden" />}>
              {conversation.topic}
            </TooltipTrigger>
            <TooltipContent side="bottom" align="start" className="max-w-sm whitespace-pre-wrap">
              {conversation.topic}
            </TooltipContent>
          </Tooltip>
        ) : null}
      </div>

      <form
        role="search"
        data-searching={searching || undefined}
        className="mr-2 shrink-0 @max-[620px]/header:mr-0 @max-[620px]/header:flex-1 @max-[620px]/header:[&:not([data-searching])]:hidden"
        onSubmit={(event) => {
          event.preventDefault()
          const text = String(new FormData(event.currentTarget).get('q') ?? '').trim()
          if (text) navigation.openSearch(text)
          else if (query) navigation.closePane()
        }}
      >
        <InputGroup className="w-56 border-0 bg-secondary/40 @max-[620px]/header:w-full dark:bg-secondary/40">
          <InputGroupAddon>
            <SearchNormal />
          </InputGroupAddon>
          {/* keyed by the query, so the field follows `?q=` (back, forward, a closed pane) without an effect */}
          <InputGroupInput key={query} ref={searchInput} name="q" type="search" defaultValue={query} placeholder="Search" aria-label={`Search in ${title}`} autoComplete="off" />
        </InputGroup>
      </form>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="hidden text-muted-foreground group-data-searching/header:inline-flex"
        aria-label="Close search"
        onClick={() => setSearching(false)}
      >
        <Xmark size={20} />
      </Button>

      <div className="flex shrink-0 items-center gap-2 group-data-searching/header:hidden max-[899px]:gap-0.5 @max-[620px]/header:gap-0.5">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="hidden text-muted-foreground @max-[620px]/header:inline-flex"
          aria-label="Search"
          onClick={() => {
            setSearching(true)
            // the field is displayed by this state change: focus it once it is
            requestAnimationFrame(() => searchInput.current?.focus())
          }}
        >
          <SearchNormal size={20} weight="Filled" />
        </Button>
        <PaneToggle label="Threads" pressed={openPane === 'threads'} className="max-[899px]:hidden" onToggle={toggle('threads')}>
          <ThreadIcon size={20} />
        </PaneToggle>
        <PaneToggle label="Pinned messages" pressed={openPane === 'pins'} className="max-[899px]:hidden" onToggle={toggle('pins')}>
          <PinIcon size={20} />
        </PaneToggle>
        <PaneToggle label="Files" pressed={openPane === 'files'} className="max-[899px]:hidden" onToggle={toggle('files')}>
          <Paperclip2 size={20} weight="Filled" />
        </PaneToggle>
        <PaneToggle label="Members" pressed={openPane === 'members'} onToggle={toggle('members')}>
          <People size={20} weight="Filled" />
        </PaneToggle>
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon" className="text-muted-foreground" aria-label={`Options for ${title}`} />}>
            <MoreH size={20} weight="Filled" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-auto min-w-48">
            <DropdownMenuItem className="min-[900px]:hidden" onClick={toggle('threads')}>
              <ThreadIcon size={16} />
              Threads
            </DropdownMenuItem>
            <DropdownMenuItem className="min-[900px]:hidden" onClick={toggle('pins')}>
              <PinIcon size={16} />
              Pinned messages
            </DropdownMenuItem>
            <DropdownMenuItem className="min-[900px]:hidden" onClick={toggle('files')}>
              <Paperclip2 />
              Files
            </DropdownMenuItem>
            <DropdownMenuSeparator className="min-[900px]:hidden" />
            <ConversationMenuItems actions={actions} placement="header" onEdit={() => setEditOpen(true)} />
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {channel ? <EditChannelDialog conversationId={conversation.id} open={editOpen} onOpenChange={setEditOpen} /> : null}
    </PaneHeader>
  )
}
