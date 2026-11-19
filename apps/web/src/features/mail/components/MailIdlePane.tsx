import { DirectboxNotif, TickCircle } from 'reicon-react'
import type { CommandId } from '@/shortcuts/commands'
import { Shortcut } from '@/shortcuts/Shortcut'

/** The keys the idle pane teaches: the ones used many times a day. */
const KEYS: Array<{ id: CommandId; label: string }> = [
  { id: 'mail.compose', label: 'New message' },
  { id: 'mail.down', label: 'Next conversation' },
  { id: 'mail.up', label: 'Previous conversation' },
  { id: 'mail.open', label: 'Open' },
  { id: 'mail.reply', label: 'Reply' },
  { id: 'mail.archive', label: 'Archive' },
  { id: 'mail.search', label: 'Search' },
]

/**
 * The reading pane with no conversation open: how much is unread (or that all is read), and the keys that make the
 * list fast. Not a dead "select a conversation".
 */
export function MailIdlePane({ unread, inbox }: { unread: number; inbox: boolean }) {
  const caughtUp = inbox && unread === 0
  const Icon = caughtUp ? TickCircle : DirectboxNotif
  return (
    <div data-slot="mail-idle" className="flex min-h-0 flex-1 flex-col items-center justify-center gap-6 p-8 text-center">
      <div className="flex flex-col items-center gap-2">
        <span className="flex size-11 items-center justify-center rounded-full bg-muted text-muted-foreground">
          <Icon className="size-5" />
        </span>
        <p className="m-0 text-sm font-medium text-foreground">
          {caughtUp ? 'You are all caught up' : unread > 0 ? `${unread > 999 ? '999+' : unread} unread` : 'No conversation open'}
        </p>
        <p className="m-0 max-w-64 text-xs text-muted-foreground">
          {caughtUp ? 'New mail shows up in the list.' : 'Pick one from the list, or use the keys below.'}
        </p>
      </div>
      <dl className="m-0 grid w-full max-w-64 grid-cols-[1fr_auto] gap-x-6 gap-y-2 rounded-xl border bg-card/50 px-4 py-3 text-left text-xs">
        {KEYS.map((key) => (
          <div key={key.id} className="contents">
            <dt className="text-muted-foreground">{key.label}</dt>
            <dd className="m-0 justify-self-end"><Shortcut id={key.id} /></dd>
          </div>
        ))}
      </dl>
    </div>
  )
}
