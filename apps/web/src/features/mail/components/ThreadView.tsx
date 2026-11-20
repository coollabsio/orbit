import { useEffect, useRef, useState, type ReactNode, type Ref } from 'react'
import { Archive, ArrowLeft, DirectInbox, More, ShieldCross, Sms as Mail, Star, Tag, Trash, Undo } from 'reicon-react'
import { Link } from 'react-router'
import { cn } from 'cn'
import type { MailLabel, MailThreadDetail } from '@orbit/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { PaneHeader } from '@/components/common/Pane'
import { Tip } from '@/components/common/Tip'
import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { printPage } from '@/lib/print'
import { folderLabel, foldedRange } from '@/features/mail/mailLib'
import type { CommandId } from '@/shortcuts/commands'
import { Shortcut } from '@/shortcuts/Shortcut'
import { ThreadReply, type ThreadReplyHandle } from '@/features/mail/compose/ThreadReply'
import { MailEmptyState } from './MailEmptyState'
import { MessageItem } from './MessageItem'
import { useMailAvatar } from './useMailAvatar'

/** Which threads of a target have a label. */
export type LabelState = 'all' | 'some' | 'none'

/** The label picker of the open thread and of the bulk bar (`l`). */
export function LabelMenu({
  labels,
  state,
  onToggle,
  open,
  onOpenChange,
  canManage,
}: {
  labels: MailLabel[]
  state: (labelId: string) => LabelState
  onToggle: (labelId: string) => void
  open: boolean
  onOpenChange: (open: boolean) => void
  canManage: boolean
}) {
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange}>
      <Tip label="Labels" side="bottom">
        <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Labels" />}>
          <Tag className="size-4" />
        </DropdownMenuTrigger>
      </Tip>
      <DropdownMenuContent align="end" className="w-auto min-w-44">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Labels</DropdownMenuLabel>
          {labels.map((label) => {
            const current = state(label.id)
            return (
              <DropdownMenuCheckboxItem
                key={label.id}
                checked={current === 'all'}
                data-partial={current === 'some' || undefined}
                onCheckedChange={() => onToggle(label.id)}
              >
                <ColorDot color={label.color} className="size-2" />
                {label.name}
              </DropdownMenuCheckboxItem>
            )
          })}
          {labels.length === 0 ? (
            canManage ? (
              <DropdownMenuItem render={<Link to="/settings/mail/labels" />}>Create labels in Settings</DropdownMenuItem>
            ) : (
              <DropdownMenuItem disabled>No labels yet</DropdownMenuItem>
            )
          ) : null}
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export interface ThreadActions {
  star: () => void
  markUnread: () => void
  archive: () => void
  spam: () => void
  trash: () => void
  restore: () => void
  toggleLabel: (labelId: string) => void
}

interface ThreadViewProps {
  detail: MailThreadDetail | undefined
  status: 'pending' | 'error' | 'success'
  onRetry: () => void
  labels: MailLabel[]
  labelsById: ReadonlyMap<string, MailLabel>
  canManage: boolean
  onBack: () => void
  actions: ThreadActions
  labelMenuOpen: boolean
  onLabelMenuOpenChange: (open: boolean) => void
  /** R / A / F act on the reply at the end of the thread. */
  replyRef?: Ref<ThreadReplyHandle>
}

function HeaderButton({ label, command, onClick, children, pressed }: { label: string; command?: CommandId; onClick: () => void; children: ReactNode; pressed?: boolean }) {
  return (
    <Tip label={command ? <span className="flex items-center gap-2">{label} <Shortcut id={command} /></span> : label} side="bottom">
      <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label={label} aria-pressed={pressed} onClick={onClick}>
        {children}
      </Button>
    </Tip>
  )
}

/** On phones, the thread's actions in one "…" menu: a row of icons does not fit next to Back. */
function PhoneThreadMenu({ thread, labels, canManage, actions }: { thread: MailThreadDetail['thread']; labels: MailLabel[]; canManage: boolean; actions: ThreadActions }) {
  const inTrash = thread.folder === 'trash'
  const inSpam = thread.folder === 'spam'
  return (
    <DropdownMenu>
      <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" className="hidden text-muted-foreground/70 max-[899px]:inline-flex" aria-label="Thread actions" />}>
        <More className="size-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-auto min-w-48">
        <DropdownMenuItem onClick={actions.star}>
          <Star className={cn(thread.starred && 'fill-[#fcd452] text-[#fcd452]')} /> {thread.starred ? 'Unstar' : 'Star'}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={actions.markUnread}>
          <Mail /> Mark as unread
        </DropdownMenuItem>
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>
            <Tag /> Labels
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent className="w-auto min-w-44">
            {labels.map((label) => (
              <DropdownMenuCheckboxItem key={label.id} checked={thread.labels.includes(label.id)} onCheckedChange={() => actions.toggleLabel(label.id)}>
                <ColorDot color={label.color} className="size-2" />
                {label.name}
              </DropdownMenuCheckboxItem>
            ))}
            {labels.length === 0 ? (
              canManage ? (
                <DropdownMenuItem render={<Link to="/settings/mail/labels" />}>Create labels in Settings</DropdownMenuItem>
              ) : (
                <DropdownMenuItem disabled>No labels yet</DropdownMenuItem>
              )
            ) : null}
          </DropdownMenuSubContent>
        </DropdownMenuSub>
        <DropdownMenuSeparator />
        {thread.folder === 'archive' ? (
          <DropdownMenuItem onClick={actions.archive}>
            <DirectInbox /> Move to inbox
          </DropdownMenuItem>
        ) : !inTrash && !inSpam ? (
          <DropdownMenuItem onClick={actions.archive}>
            <Archive /> Archive
          </DropdownMenuItem>
        ) : null}
        {!inTrash ? (
          <DropdownMenuItem onClick={actions.spam}>
            {inSpam ? <DirectInbox /> : <ShieldCross />} {inSpam ? 'Not spam' : 'Report spam'}
          </DropdownMenuItem>
        ) : null}
        {inTrash ? (
          <DropdownMenuItem onClick={actions.restore}>
            <Undo /> Restore
          </DropdownMenuItem>
        ) : (
          <DropdownMenuItem variant="destructive" onClick={actions.trash}>
            <Trash /> Move to trash
          </DropdownMenuItem>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** The reading pane: the thread's actions, then its messages, oldest first; only the newest is open at first. */
export function ThreadView({ detail, status, onRetry, labels, labelsById, canManage, onBack, actions, labelMenuOpen, onLabelMenuOpenChange, replyRef }: ThreadViewProps) {
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})
  /** The middle of a long thread is shown (it folds into "N more messages" at first). */
  const [showAll, setShowAll] = useState(false)
  // the fold button goes away on click: the focus moves to the first message it showed
  const messagesRef = useRef<HTMLDivElement>(null)
  const focusFrom = useRef<number | null>(null)
  useEffect(() => {
    if (!showAll || focusFrom.current === null) return
    // the header of that message (its first `role=button`; the body may hold others)
    const shown = messagesRef.current?.querySelectorAll('[data-slot="mail-message"]')[focusFrom.current]?.querySelector<HTMLElement>('[role="button"]')
    focusFrom.current = null
    shown?.focus()
  }, [showAll])
  const avatarOf = useMailAvatar()
  if (!detail) {
    return (
      <>
        <PaneHeader>
          <Button variant="ghost" size="icon-sm" className="hidden text-muted-foreground/70 max-[899px]:inline-flex" aria-label="Back" onClick={onBack}>
            <ArrowLeft className="size-4" />
          </Button>
        </PaneHeader>
        {status === 'error' ? (
          <div className="flex min-h-0 flex-1 flex-col">
            <MailEmptyState
              icon={Mail}
              title="This conversation could not be loaded"
              description="It may have been deleted, or the connection failed."
              action={
                <Button variant="outline" size="sm" onClick={onRetry}>
                  Try again
                </Button>
              }
            />
          </div>
        ) : (
          <div className="mx-auto flex w-full max-w-[800px] flex-col gap-3 px-5 pt-6">
            <Skeleton className="h-7 w-2/3" />
            <Skeleton className="h-40 w-full" />
          </div>
        )}
      </>
    )
  }

  const { thread, messages } = detail
  const lastIndex = messages.length - 1
  const subject = thread.subject || '(no subject)'
  const chips = thread.labels.flatMap((id) => labelsById.get(id) ?? [])
  const inTrash = thread.folder === 'trash'
  const inSpam = thread.folder === 'spam'
  const range = showAll ? null : foldedRange(messages.length)
  // a message opened by hand stays out of the fold
  const fold = range && !messages.slice(range[0], range[1]).some((message) => expanded[message.id]) ? range : null

  return (
    <>
      <PaneHeader>
        <Button variant="ghost" size="icon-sm" className="hidden text-muted-foreground/70 max-[899px]:inline-flex" aria-label="Back" onClick={onBack}>
          <ArrowLeft className="size-4" />
        </Button>
        <span className="flex-1" />
        <div className="contents max-[899px]:hidden">
        <HeaderButton label={thread.starred ? 'Unstar' : 'Star'} command="mail.star" pressed={thread.starred} onClick={actions.star}>
          <Star className={cn('size-4', thread.starred && 'fill-[#fcd452] text-[#fcd452]')} />
        </HeaderButton>
        <HeaderButton label="Mark as unread" command="mail.read" onClick={actions.markUnread}>
          <Mail className="size-4" />
        </HeaderButton>
        <LabelMenu
          labels={labels}
          state={(id) => (thread.labels.includes(id) ? 'all' : 'none')}
          onToggle={actions.toggleLabel}
          open={labelMenuOpen}
          onOpenChange={onLabelMenuOpenChange}
          canManage={canManage}
        />
        {thread.folder === 'archive' ? (
          <HeaderButton label="Move to inbox" command="mail.archive" onClick={actions.archive}>
            <DirectInbox className="size-4" />
          </HeaderButton>
        ) : !inTrash && !inSpam ? (
          <HeaderButton label="Archive" command="mail.archive" onClick={actions.archive}>
            <Archive className="size-4" />
          </HeaderButton>
        ) : null}
        {!inTrash ? (
          <HeaderButton label={inSpam ? 'Not spam' : 'Report spam'} command="mail.spam" onClick={actions.spam}>
            {inSpam ? <DirectInbox className="size-4" /> : <ShieldCross className="size-4" />}
          </HeaderButton>
        ) : null}
        {inTrash ? (
          <HeaderButton label="Restore" onClick={actions.restore}>
            <Undo className="size-4" />
          </HeaderButton>
        ) : (
          <HeaderButton label="Move to trash" command="mail.trash" onClick={actions.trash}>
            <Trash className="size-4" />
          </HeaderButton>
        )}
        </div>
        <PhoneThreadMenu thread={thread} labels={labels} canManage={canManage} actions={actions} />
      </PaneHeader>
      <div data-print-root="" className="min-h-0 flex-1 overflow-y-auto">
        <div ref={messagesRef} className="mx-auto flex max-w-[800px] flex-col px-5 pt-6 pb-8">
          <div className="flex flex-wrap items-center gap-2.5 pb-2">
            <h1 className="m-0 text-xl font-semibold break-words text-foreground">{subject}</h1>
            <Badge variant="secondary">{folderLabel(thread.folder)}</Badge>
            {chips.map((label) => (
              <Badge key={label.id} variant="outline">
                <ColorDot color={label.color} />
                {label.name}
              </Badge>
            ))}
          </div>
          {inSpam ? (
            <Alert data-kind="spam" data-print-hide="" className="mt-2">
              <ShieldCross />
              <AlertTitle>In Spam</AlertTitle>
              <AlertDescription>Links and images in spam can be harmful. Move it to the inbox only if you trust the sender.</AlertDescription>
              <AlertAction>
                <Button variant="outline" size="sm" onClick={actions.spam}>
                  Not spam
                </Button>
              </AlertAction>
            </Alert>
          ) : null}
          {inTrash ? (
            <Alert data-kind="trash" data-print-hide="" className="mt-2">
              <Trash />
              <AlertTitle>In Trash</AlertTitle>
              <AlertDescription>Conversations in the trash are deleted for good after 30 days.</AlertDescription>
              <AlertAction>
                <Button variant="outline" size="sm" onClick={actions.restore}>
                  Restore
                </Button>
              </AlertAction>
            </Alert>
          ) : null}
          {messages.map((message, index) => {
            // a long thread folds its middle (not the ones the user opened)
            if (fold && index >= fold[0] && index < fold[1]) {
              return index === fold[0] ? (
                <button
                  key="fold"
                  type="button"
                  data-slot="mail-thread-fold"
                  data-print-hide=""
                  className="group/fold relative my-1 flex h-8 w-full items-center justify-center text-xs text-muted-foreground outline-none before:absolute before:inset-x-0 before:top-1/2 before:border-t before:border-dashed hover:text-foreground focus-visible:text-foreground"
                  onClick={() => {
                    focusFrom.current = fold[0]
                    setShowAll(true)
                  }}
                >
                  <span className="relative rounded-full border bg-background px-3 py-1 transition-colors group-hover/fold:bg-muted group-focus-visible/fold:ring-2 group-focus-visible/fold:ring-ring/50">
                    {fold[1] - fold[0]} more messages
                  </span>
                </button>
              ) : null
            }
            const open = expanded[message.id] ?? index === lastIndex
            return (
              <MessageItem
                key={message.id}
                threadId={thread.id}
                message={message}
                expanded={open}
                avatarOf={avatarOf}
                onToggle={() => setExpanded((previous) => ({ ...previous, [message.id]: !open }))}
                onPrint={() => printPage(subject)}
              />
            )
          })}
          <ThreadReply ref={replyRef} threadId={thread.id} last={messages.at(-1)} />
        </div>
      </div>
    </>
  )
}
