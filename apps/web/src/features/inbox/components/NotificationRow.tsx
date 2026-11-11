import { Check, Import } from 'reicon-react'
import type { NotificationRecord } from '@orbit/api/generated/types.gen'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { ProfileTrigger } from '@/components/common/ProfileTrigger'
import { relativeTime } from '@/lib/format'
import { notificationCopy } from '@/features/inbox/api'
import type { useNotificationRows } from '@/features/inbox/useNotificationRows'

type Rows = ReturnType<typeof useNotificationRows>

/** One notification: unread dot, actor, what happened and when. On hover, an unread row offers "Mark as read". */
export function NotificationRow({ notification, rows, onOpen }: { notification: NotificationRecord; rows: Rows; /** After the row opens its target (the popover closes). */ onOpen?: () => void }) {
  const actor = rows.actor(notification)
  const copy = notificationCopy(notification, rows.pageTitle(notification), actor?.name)
  const unread = !notification.read_at
  return (
    // the avatar and the read button are beside the row button, not inside it: a button cannot hold another one
    <div data-slot="notification-row" data-unread={unread || undefined} className="group/row relative">
      <Button
        variant="ghost"
        className="h-auto min-h-14 w-full min-w-0 justify-start gap-2.5 rounded-none border-x-0 border-t-0 border-b-border px-3 py-1.5 text-left font-normal"
        onClick={() => {
          rows.open(notification)
          onOpen?.()
        }}
      >
        <span className="flex w-2 shrink-0 justify-center">
          {unread ? <span className="size-2 shrink-0 rounded-full bg-primary" /> : null}
        </span>
        <span aria-hidden="true" className="size-7 shrink-0" />
        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="truncate text-[0.8125rem] text-foreground data-unread:font-semibold" data-unread={unread || undefined}>
            {copy.title}
          </span>
          <span className="truncate text-xs text-muted-foreground/70">{actor && !notification.import_id ? `${actor.name} · ${copy.body}` : copy.body}</span>
        </span>
        <span
          className="shrink-0 text-xs text-muted-foreground/70 tabular-nums transition-opacity duration-100 group-has-[[data-slot=notification-read]:focus-visible]/row:opacity-0 data-unread:hover-fine:group-hover/row:opacity-0"
          data-unread={unread || undefined}
        >
          {relativeTime(notification.created_at)}
        </span>
      </Button>
      {/* over the spacer: 12px padding, the 8px unread column and the 10px gap; an import notice is the user's own */}
      {notification.import_id ? (
        <span aria-hidden="true" className="pointer-events-none absolute top-1/2 left-[30px] flex size-7 -translate-y-1/2 items-center justify-center rounded-full bg-muted">
          <Import className="size-4 text-muted-foreground" />
        </span>
      ) : (
        <ProfileTrigger userId={actor?.id} name={actor?.name ?? 'Unknown'} kind="avatar" className="absolute top-1/2 left-[30px] flex -translate-y-1/2">
          <Avatar className="size-7">
            <AvatarFallback>{(actor?.name ?? '?').charAt(0).toUpperCase()}</AvatarFallback>
          </Avatar>
        </ProfileTrigger>
      )}
      {/* over the time on hover: a pointer that reaches the row can clear it without opening it; touch opens or marks all */}
      {unread ? (
        <Tip label="Mark as read">
          <Button
            data-slot="notification-read"
            variant="ghost"
            size="icon-xs"
            aria-label="Mark as read"
            className="absolute top-1/2 right-2.5 -translate-y-1/2 pointer-events-none opacity-0 transition-opacity duration-100 focus-visible:pointer-events-auto focus-visible:opacity-100 hover-fine:group-hover/row:pointer-events-auto hover-fine:group-hover/row:opacity-100"
            onClick={() => rows.markRead(notification)}
          >
            <Check />
          </Button>
        </Tip>
      ) : null}
    </div>
  )
}
