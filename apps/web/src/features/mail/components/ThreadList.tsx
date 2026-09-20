import { Edit as SquarePen } from 'reicon-react'
import { useNavigate } from 'react-router'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/common/EmptyState'
import { setThreadRead } from '@/mock/actions'
import type { MailFolder, MailThread } from '@/mock/types'
import { FOLDER_ICONS } from '@/features/mail/mailLib'
import { ThreadRow } from './ThreadRow'

interface ThreadListProps {
  folders: MailFolder[]
  folder: MailFolder
  threads: MailThread[]
  activeThreadId: string | undefined
  onCompose: () => void
}

export function ThreadList({ folders, folder, threads, activeThreadId, onCompose }: ThreadListProps) {
  const navigate = useNavigate()
  const FolderIcon = FOLDER_ICONS[folder.icon]

  const openThread = (threadId: string) => {
    setThreadRead(threadId, true)
    navigate(`/mail/${threadId}?folder=${folder.id}`)
  }

  return (
    <section className="flex h-full w-[360px] shrink-0 flex-col border-l border-border bg-background max-[1199px]:w-[300px] max-[899px]:w-auto max-[899px]:flex-1 max-[899px]:border-l-0 max-[899px]:group-data-[view=thread]/mail:hidden">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3 max-[899px]:border-b-0">
        <span className="truncate text-[13px] font-semibold text-foreground max-[899px]:hidden">{folder.name}</span>
        <span className="hidden truncate text-[13px] font-semibold text-foreground max-[899px]:block">Mail</span>
        <span className="flex-1" />
        <Button className="max-[899px]:hidden" onClick={onCompose}>
          <SquarePen className="size-4" />
          Compose
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          className="hidden text-muted-foreground/70 max-[899px]:inline-flex"
          aria-label="Compose"
          onClick={onCompose}
        >
          <SquarePen className="size-4" />
        </Button>
      </div>
      <div className="hidden min-h-10 items-center gap-1.5 overflow-x-auto border-b border-border px-3 py-1 [scrollbar-width:none] max-[899px]:flex">
        {folders.map((f) => (
          <Button
            key={f.id}
            type="button"
            variant="ghost"
            className="inline-flex h-7 shrink-0 items-center gap-1 rounded-md border-0 px-2.5 text-[13px] font-medium whitespace-nowrap text-muted-foreground transition-colors hover:bg-accent hover:text-foreground dark:hover:bg-accent data-[active]:bg-primary/10 data-[active]:text-primary data-[active]:ring-1 data-[active]:ring-primary/25 data-[active]:ring-inset"
            data-active={f.id === folder.id || undefined}
            onClick={() => navigate(`/mail?folder=${f.id}`)}
          >
            {f.name}
          </Button>
        ))}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {threads.length === 0 ? (
          <EmptyState
            icon={FolderIcon}
            title="Nothing here"
            description={`No conversations in ${folder.name}.`}
          />
        ) : (
          threads.map((thread) => (
            <ThreadRow
              key={thread.id}
              thread={thread}
              active={thread.id === activeThreadId}
              onOpen={openThread}
            />
          ))
        )}
      </div>
    </section>
  )
}
