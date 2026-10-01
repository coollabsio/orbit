import { ArrowLeft, Bell, BellOff, Maximize4, Minimize, MoreH, Xmark } from 'reicon-react'
import { toast } from 'sonner'
import { PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from '@/components/ui/breadcrumb'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { useSetThreadFollow } from '../../api/mutations'
import { threadPath } from '../../chatRoutes'
import { useChatNavigation } from '../../useChatNavigation'

function FollowButton({ rootId, following }: { rootId: string; following: boolean }) {
  const { mutate: setFollow, isPending } = useSetThreadFollow()
  const label = following ? 'Unfollow thread' : 'Follow thread'
  return (
    <Button
      variant="ghost"
      size="icon"
      aria-label={label}
      aria-pressed={following}
      title={following ? 'Following. You are notified of new replies.' : 'Follow to be notified of new replies'}
      disabled={isPending}
      onClick={() =>
        setFollow(
          { rootId, following: !following },
          { onError: () => toast.error(following ? 'Could not unfollow the thread. Try again.' : 'Could not follow the thread. Try again.') },
        )
      }
    >
      {following ? <Bell weight="Filled" className="size-5 text-primary" /> : <BellOff weight="Filled" className="size-5" />}
    </Button>
  )
}

interface ThreadHeaderProps {
  conversationId: string
  rootId: string
  variant: 'pane' | 'full'
  /** `#channel`, or the names of a DM. */
  conversationTitle: string
  /** The first line of the root message, already cut. */
  rootLine: string
  following: boolean
  /** Shows expand (pane) or collapse (full). */
  canResize: boolean
  /** Runs before expand or collapse navigates, to hand over the scroll position. */
  onResize: () => void
  onClose: () => void
}

/** The 48px header of a thread. Pane: "Thread", expand, follow, close. Full view: breadcrumb, collapse, follow, `…`. */
export function ThreadHeader({
  conversationId,
  rootId,
  variant,
  conversationTitle,
  rootLine,
  following,
  canResize,
  onResize,
  onClose,
}: ThreadHeaderProps) {
  const { expandThread, collapseThread, openConversation } = useChatNavigation()

  if (variant === 'pane') {
    return (
      <PaneHeader data-slot="thread-header" className="gap-1">
        <PaneTitle render={<h2 />} className="flex-1">
          Thread
        </PaneTitle>
        {canResize ? (
          <Button
            variant="ghost"
            size="icon"
            aria-label="Open thread in full view"
            title="Open in full view"
            onClick={() => {
              onResize()
              expandThread(rootId)
            }}
          >
            <Maximize4 weight="Filled" className="size-5" />
          </Button>
        ) : null}
        <FollowButton rootId={rootId} following={following} />
        <Button variant="ghost" size="icon" aria-label="Close thread" title="Close thread" onClick={onClose}>
          <Xmark weight="Filled" className="size-5" />
        </Button>
      </PaneHeader>
    )
  }

  return (
    <PaneHeader data-slot="thread-header" className="gap-1">
      <Button variant="ghost" size="icon" className="min-[900px]:hidden" aria-label="Back" onClick={onClose}>
        <ArrowLeft weight="Filled" className="size-5" />
      </Button>
      <Breadcrumb className="min-w-0 flex-1">
        <BreadcrumbList className="flex-nowrap text-[13px]">
          <BreadcrumbItem className="shrink-0">
            {/* The channel crumb opens the channel at the root message. */}
            <BreadcrumbLink
              render={<button type="button" onClick={() => openConversation(conversationId, rootId)} />}
              className="rounded-sm font-semibold text-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
            >
              {conversationTitle}
            </BreadcrumbLink>
          </BreadcrumbItem>
          <BreadcrumbSeparator className="shrink-0" />
          <BreadcrumbItem className="min-w-0">
            <BreadcrumbPage className="truncate">
              <span className="font-semibold">Thread</span>
              {rootLine ? <span className="text-muted-foreground"> · “{rootLine}”</span> : null}
            </BreadcrumbPage>
          </BreadcrumbItem>
        </BreadcrumbList>
      </Breadcrumb>
      {canResize ? (
        <Button
          variant="ghost"
          size="icon"
          className="max-[899px]:hidden"
          aria-label="Show thread in the side pane"
          title="Show in the side pane"
          onClick={() => {
            onResize()
            collapseThread(rootId)
          }}
        >
          <Minimize weight="Filled" className="size-5" />
        </Button>
      ) : null}
      <FollowButton rootId={rootId} following={following} />
      <DropdownMenu>
        <DropdownMenuTrigger render={<Button variant="ghost" size="icon" aria-label="Thread actions" title="Thread actions" />}>
          <MoreH weight="Filled" className="size-5" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-auto min-w-44">
          <DropdownMenuItem
            onClick={() => {
              navigator.clipboard.writeText(`${window.location.origin}${threadPath(conversationId, rootId)}`).then(
                () => toast.success('Link copied'),
                () => toast.error('Could not copy. Try again.'),
              )
            }}
          >
            Copy link
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => openConversation(conversationId, rootId)}>Show in {conversationTitle}</DropdownMenuItem>
          <DropdownMenuItem onClick={onClose}>Close thread</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </PaneHeader>
  )
}
