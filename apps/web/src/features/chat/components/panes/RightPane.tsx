import { useEffect, useState, type ComponentProps, type ComponentType, type ReactNode } from 'react'
import { cn } from 'cn'
import { Xmark } from 'reicon-react'
import { PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'
import { Spinner } from '@/components/ui/spinner'
import { useChatNavigation } from '@/features/chat/useChatNavigation'

interface RightPaneProps {
  title: string
  /** Controls in the header, between the title and the close button. */
  actions?: ReactNode
  children: ReactNode
}

/** The frame of a right pane: 48px header with the title and a close button, and Escape closes the pane. */
function RightPane({ title, actions, children }: RightPaneProps) {
  const { closePane } = useChatNavigation()
  return (
    <section
      data-slot="chat-right-pane"
      aria-label={title}
      className="flex h-full min-h-0 flex-col"
      onKeyDown={(event) => {
        if (event.key !== 'Escape' || event.defaultPrevented) return
        // React events also bubble out of portals: Escape in a menu, popover or dialog opened from the pane closes that one, not the pane.
        if (!(event.target instanceof Node) || !event.currentTarget.contains(event.target)) return
        event.preventDefault()
        closePane()
      }}
    >
      <PaneHeader>
        <PaneTitle render={<h2 />}>{title}</PaneTitle>
        <div className="ml-auto flex items-center gap-2">
          {actions}
          <Button variant="ghost" size="icon" aria-label={`Close ${title.toLowerCase()}`} title="Close" onClick={closePane}>
            <Xmark weight="Filled" className="size-5" />
          </Button>
        </div>
      </PaneHeader>
      {children}
    </section>
  )
}

/** The scrolling part of a pane, with 8px padding, so a list never starts flush at the top. */
function RightPaneBody({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="chat-right-pane-body" className={cn('flex min-h-0 flex-1 flex-col overflow-y-auto p-2', className)} {...props} />
}

/** A blank pane while it loads; a quiet spinner only when loading takes longer than 300ms. */
function RightPaneLoading() {
  const [slow, setSlow] = useState(false)
  useEffect(() => {
    const timer = setTimeout(() => setSlow(true), 300)
    return () => clearTimeout(timer)
  }, [])
  return (
    <div data-slot="chat-right-pane-loading" aria-busy="true" className="flex flex-1 items-center justify-center">
      {slow ? <Spinner className="text-muted-foreground" /> : null}
    </div>
  )
}

/** A failed read: what failed, and "Try again". */
function RightPaneError({ title, onRetry }: { title: string; onRetry: () => void }) {
  return (
    <Empty role="alert">
      <EmptyHeader>
        <EmptyTitle>{title}</EmptyTitle>
        <EmptyDescription>Check your connection and try again.</EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        <Button variant="outline" size="sm" onClick={onRetry}>
          Try again
        </Button>
      </EmptyContent>
    </Empty>
  )
}

interface RightPaneEmptyProps {
  icon: ComponentType<{ className?: string; weight?: 'Filled' | 'Outline' }>
  title: string
  /** Names the next action. */
  description?: string
  action?: ReactNode
}

/** An empty state that fills the pane. */
function RightPaneEmpty({ icon: Icon, title, description, action }: RightPaneEmptyProps) {
  return (
    <Empty>
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <Icon weight="Filled" />
        </EmptyMedia>
        <EmptyTitle>{title}</EmptyTitle>
        {description ? <EmptyDescription>{description}</EmptyDescription> : null}
      </EmptyHeader>
      {action ? <EmptyContent>{action}</EmptyContent> : null}
    </Empty>
  )
}

export { RightPane, RightPaneBody, RightPaneEmpty, RightPaneError, RightPaneLoading }
