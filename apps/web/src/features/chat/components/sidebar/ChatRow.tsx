import type { ComponentProps } from 'react'
import { Link } from 'react-router'
import { ArrowLeft } from 'reicon-react'
import { cn } from 'cn'
import { buttonVariants } from '@/components/ui/button'
import { CHAT_PATH } from '@/features/chat/chatRoutes'

/**
 * A 32px row of the chat sidebar: a link (`ChatRowLink`) and, at its end, actions and the unread marker or count.
 * State is in `data-active`, `data-unread` and `data-muted`. Rows do not animate. With a chat theme, the open row takes
 * the theme's selected colour and its text takes the readable colour for it (without a theme both variables are unset,
 * so the row keeps the inherited tokens).
 */
export function ChatRow({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="chat-row"
      className={cn(
        'group/row relative flex h-8 min-w-0 shrink-0 items-center rounded-md text-[13px] text-foreground/80 has-[a:focus-visible]:ring-2 has-[a:focus-visible]:ring-ring/50 has-[a:focus-visible]:ring-inset data-[active=true]:bg-[var(--chat-selected,var(--muted))] data-[active=true]:[--foreground:var(--chat-selected-foreground)] data-[active=true]:[--muted-foreground:var(--chat-selected-foreground)] data-[active=true]:text-foreground hover-fine:not-data-[active=true]:hover:bg-muted/60 max-[899px]:h-10 max-[899px]:text-sm',
        className,
      )}
      {...props}
    />
  )
}

/** The link of a row: leading icon and name. Middle click and the keyboard work as for any link. */
export function ChatRowLink({ className, ...props }: ComponentProps<typeof Link>) {
  return <Link data-slot="chat-row-link" className={cn('flex h-full min-w-0 flex-1 items-center gap-2 rounded-md pl-2 outline-none', className)} {...props} />
}

/** The name in a row: bold when unread, muted when the conversation is muted. */
export function ChatRowName({ className, ...props }: ComponentProps<'span'>) {
  return (
    <span
      data-slot="chat-row-name"
      className={cn('min-w-0 truncate group-data-[muted=true]/row:text-muted-foreground/70 group-data-[unread=true]/row:font-semibold group-data-[unread=true]/row:text-foreground', className)}
      {...props}
    />
  )
}

/** The end of a row: the `…` button, then the unread marker or count. */
export function ChatRowEnd({ className, ...props }: ComponentProps<'span'>) {
  return <span data-slot="chat-row-end" className={cn('flex shrink-0 items-center gap-1.5 pr-2 pl-1', className)} {...props} />
}

/** The unread marker: a 6px pink square, the pixel of the Orbit logo. Always next to bold text, never the only sign. */
export function UnreadMarker({ className, ...props }: ComponentProps<'span'>) {
  return <span data-slot="unread-marker" aria-hidden="true" className={cn('size-1.5 shrink-0 bg-primary', className)} {...props} />
}

/** The pink count badge. The row's accessible name carries the number. */
export function CountBadge({ count, className }: { count: number; className?: string }) {
  return (
    <span
      data-slot="count-badge"
      aria-hidden="true"
      className={cn('inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-full bg-primary px-1 text-[10px] leading-none font-semibold text-primary-foreground tabular-nums', className)}
    >
      {count > 99 ? '99+' : count}
    </span>
  )
}

/** Phones only: back from a conversation, a view or a thread to the chat list. */
export function ChatBackLink({ className }: { className?: string }) {
  return (
    <Link
      data-slot="chat-back-link"
      to={CHAT_PATH}
      aria-label="Back to conversations"
      className={cn(buttonVariants({ variant: 'ghost', size: 'icon' }), 'hidden text-muted-foreground max-[899px]:inline-flex', className)}
    >
      <ArrowLeft className="size-5" />
    </Link>
  )
}
