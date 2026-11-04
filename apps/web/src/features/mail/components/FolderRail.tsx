import type { ComponentType, ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { DirectboxDefault, Star } from 'reicon-react'
import type { MailCounts, MailLabel, MailMeMailbox } from '@orbit/api/generated/types.gen'
import { ColorDot } from '@/components/common/ColorDot'
import { PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { FOLDERS, mailViewPath, type MailView } from '@/features/mail/mailLib'

interface FolderRailProps {
  view: MailView
  mailboxes: MailMeMailbox[]
  labels: MailLabel[]
  counts: MailCounts | undefined
}

function RailRow({
  icon: Icon,
  label,
  unread = 0,
  active,
  onClick,
  children,
}: {
  icon?: ComponentType<{ className?: string }>
  label: string
  unread?: number
  active: boolean
  onClick: () => void
  children?: ReactNode
}) {
  return (
    <Button
      type="button"
      variant="ghost"
      className="w-full min-w-0 justify-start gap-2.5 text-[13px] data-active:bg-sidebar-accent data-active:text-sidebar-accent-foreground"
      data-active={active || undefined}
      aria-current={active ? 'page' : undefined}
      aria-label={unread > 0 ? `${label}, ${unread} unread` : label}
      onClick={onClick}
    >
      {Icon ? <Icon className="opacity-90" /> : children}
      <span className="min-w-0 flex-1 truncate text-left">{label}</span>
      {unread > 0 ? (
        <Badge aria-hidden="true" className="h-4 min-w-4 px-1 text-[10px] tabular-nums">
          {unread > 999 ? '999+' : unread}
        </Badge>
      ) : null}
    </Button>
  )
}

function RailSection({ children }: { children: string }) {
  return <span className="mt-3 px-2.5 py-1 text-[11px] font-medium text-sidebar-foreground/60 select-none">{children}</span>
}

/**
 * The mail sidebar: the inbox of every mailbox, each mailbox with its unread count (when there are several), Starred,
 * the folders and the labels. With a mailbox open, the folders are that mailbox's.
 */
export function FolderRail({ view, mailboxes, labels, counts }: FolderRailProps) {
  const navigate = useNavigate()
  const unread = (mailboxId: string | undefined, folder: 'inbox' | 'spam') =>
    (counts?.mailboxes ?? []).filter((mailbox) => !mailboxId || mailbox.mailbox_id === mailboxId).reduce((sum, mailbox) => sum + mailbox[folder], 0)
  const scope = view.kind === 'folder' ? view.mailboxId : undefined
  const go = (next: MailView) => navigate(mailViewPath(next))

  return (
    <nav aria-label="Mail folders" className="flex h-full w-[200px] shrink-0 flex-col bg-background max-[899px]:hidden">
      <PaneHeader>
        <PaneTitle>Mail</PaneTitle>
      </PaneHeader>
      <div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto p-2">
        <RailRow
          icon={FOLDERS[0].icon}
          label="Inbox"
          unread={unread(undefined, 'inbox')}
          active={view.kind === 'folder' && view.folder === 'inbox' && !view.mailboxId}
          onClick={() => go({ kind: 'folder', folder: 'inbox' })}
        />
        {mailboxes.length > 1
          ? mailboxes.map((mailbox) => (
              <RailRow
                key={mailbox.id}
                icon={DirectboxDefault}
                label={mailbox.name}
                unread={unread(mailbox.id, 'inbox')}
                active={view.kind === 'folder' && view.folder === 'inbox' && view.mailboxId === mailbox.id}
                onClick={() => go({ kind: 'folder', folder: 'inbox', mailboxId: mailbox.id })}
              />
            ))
          : null}
        <RailRow icon={Star} label="Starred" active={view.kind === 'starred'} onClick={() => go({ kind: 'starred' })} />
        {FOLDERS.slice(1).map(({ folder, label, icon }) => (
          <RailRow
            key={folder}
            icon={icon}
            label={label}
            unread={folder === 'spam' ? unread(scope, 'spam') : 0}
            active={view.kind === 'folder' && view.folder === folder}
            onClick={() => go({ kind: 'folder', folder, mailboxId: scope })}
          />
        ))}
        {labels.length > 0 ? <RailSection>Labels</RailSection> : null}
        {labels.map((label) => (
          <RailRow
            key={label.id}
            label={label.name}
            active={view.kind === 'label' && view.labelId === label.id}
            onClick={() => go({ kind: 'label', labelId: label.id })}
          >
            <span className="flex size-4 items-center justify-center">
              <ColorDot color={label.color} className="size-2" />
            </span>
          </RailRow>
        ))}
      </div>
    </nav>
  )
}
