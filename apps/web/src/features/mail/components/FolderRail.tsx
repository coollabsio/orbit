import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Add as Plus } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { createMailFolder, moveThread } from '@/mock/actions'
import type { MailFolder, MailThread } from '@/mock/types'
import { FOLDER_ICONS, folderUnreadCount } from '@/features/mail/mailLib'

interface FolderRailProps {
  folders: MailFolder[]
  threads: MailThread[]
  activeFolderId: string
}

export function FolderRail({ folders, threads, activeFolderId }: FolderRailProps) {
  const navigate = useNavigate()
  const [adding, setAdding] = useState(false)
  const [dropFolderId, setDropFolderId] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const system = folders.filter((f) => !f.custom)
  const custom = folders.filter((f) => f.custom)

  const commit = () => {
    const name = draft.trim()
    setAdding(false)
    setDraft('')
    if (!name) return
    const folder = createMailFolder(name)
    navigate(`/mail?folder=${folder.id}`)
  }

  const row = (folder: MailFolder) => {
    const Icon = FOLDER_ICONS[folder.icon]
    const unread = folderUnreadCount(threads, folder.id)
    // "Starred" is virtual (filters on the star flag): it cannot hold a dropped thread
    const droppable = folder.id !== 'f_starred'
    return (
      <Button
        key={folder.id}
        type="button"
        variant="ghost"
        className="relative flex h-8 w-full min-w-0 items-center justify-start gap-2.5 overflow-hidden rounded-md border-0 px-2.5 text-[13px] font-medium whitespace-nowrap text-sidebar-foreground transition-colors hover:bg-sidebar-accent/50 hover:text-sidebar-foreground dark:hover:bg-sidebar-accent/50 data-[active]:bg-sidebar-accent data-[active]:text-sidebar-accent-foreground data-[drop-over]:bg-primary/10 data-[drop-over]:text-primary"
        data-active={folder.id === activeFolderId || undefined}
        data-drop-over={dropFolderId === folder.id || undefined}
        onClick={() => navigate(`/mail?folder=${folder.id}`)}
        onDragOver={(e) => {
          if (!droppable || !e.dataTransfer.types.includes('text/mail-thread-id')) return
          e.preventDefault()
          e.dataTransfer.dropEffect = 'move'
          if (dropFolderId !== folder.id) setDropFolderId(folder.id)
        }}
        onDragLeave={() => {
          if (dropFolderId === folder.id) setDropFolderId(null)
        }}
        onDrop={(e) => {
          e.preventDefault()
          setDropFolderId(null)
          const threadId = e.dataTransfer.getData('text/mail-thread-id')
          if (threadId && droppable) moveThread(threadId, folder.id)
        }}
      >
        <Icon className="size-4 shrink-0 opacity-90" />
        <span className="min-w-0 flex-1 truncate text-left">{folder.name}</span>
        {unread > 0 ? (
          <Badge className="inline-flex h-4 min-w-4 items-center justify-center rounded-full border-0 bg-primary px-1 text-[10px] font-semibold text-primary-foreground">
            {unread}
          </Badge>
        ) : null}
      </Button>
    )
  }

  return (
    <nav className="flex h-full w-[200px] shrink-0 flex-col bg-background max-[899px]:hidden">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
        <span className="truncate text-[13px] font-semibold text-foreground">Mail</span>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto p-2">
        {system.map(row)}
        <div className="mt-3 flex items-center pr-0.5">
          <span className="px-2.5 py-1 text-[11px] font-medium text-sidebar-foreground/60 select-none">
            Custom folders
          </span>
          <span className="flex-1" />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="text-muted-foreground/70"
            aria-label="New folder"
            title="New folder"
            onClick={() => setAdding(true)}
          >
            <Plus className="size-4" />
          </Button>
        </div>
        {custom.map(row)}
        {adding ? (
          <Input
            className="mt-0.5 h-7 text-[13px] md:text-[13px]"
            autoFocus
            placeholder="Folder name"
            aria-label="New folder name"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                commit()
              } else if (e.key === 'Escape') {
                setDraft('')
                setAdding(false)
              }
            }}
          />
        ) : null}
      </div>
    </nav>
  )
}
