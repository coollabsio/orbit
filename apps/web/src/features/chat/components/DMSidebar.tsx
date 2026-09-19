import { useEffect, useState } from 'react'
import { SquarePen } from 'lucide-react'
import { useNavigate } from 'react-router'
import { Button } from '@/components/ui/button'
import { UserAvatar } from '../../../components/ui/UserAvatar'
import type { AppState } from '../../../mock/types'
import { relativeTime } from '../../../lib/format'
import { extractPreview } from '../chatLib'
import { NewDMModal } from './NewDMModal'

const WIDTH_KEY = 'orbit:dm_sidebar_width'
const MIN_WIDTH = 220
const MAX_WIDTH = 420

function initialWidth() {
  const stored = Number(window.localStorage.getItem(WIDTH_KEY))
  return Number.isFinite(stored) && stored > 0 ? Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, stored)) : 240
}

export function DMSidebar({ state, activeId }: { state: AppState; activeId: string | null }) {
  const navigate = useNavigate()
  const [creating, setCreating] = useState(false)
  const [width, setWidth] = useState(initialWidth)

  useEffect(() => {
    window.localStorage.setItem(WIDTH_KEY, String(width))
  }, [width])

  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault()
    const startX = event.clientX
    const startWidth = width
    const move = (moveEvent: PointerEvent) => setWidth(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, startWidth + moveEvent.clientX - startX)))
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  return (
    <>
      <aside className="relative flex w-60 shrink-0 flex-col border-r border-border bg-sidebar text-sidebar-foreground max-[899px]:w-full! max-[899px]:group-data-[view=conversation]/chat:hidden" style={{ width }}>
        <div className="absolute top-0 -right-px bottom-0 z-20 w-[5px] cursor-col-resize transition-colors hover:bg-primary/40 max-[899px]:hidden" onPointerDown={startResize} />
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
          <span className="text-[13px] font-semibold text-foreground">Direct Messages</span>
          <span className="flex-1" />
          <Button type="button" variant="ghost" size="icon-sm" aria-label="New direct message" onClick={() => setCreating(true)}>
            <SquarePen className="size-4" />
          </Button>
        </div>
        <div className="flex-1 overflow-y-auto p-2 max-[899px]:px-1.5">
          {state.directMessages.map((dm) => {
            const user = state.users.find((candidate) => candidate.id === dm.participantId)
            if (!user) return null
            const last = state.chatMessages.filter((message) => message.channelId === dm.id && !message.threadRootId).at(-1)
            return (
              <button
                key={dm.id}
                type="button"
                className="group/dmrow mb-0.5 flex w-full min-w-0 items-center gap-2.5 rounded-lg border border-transparent p-2 text-left text-muted-foreground transition-colors hover:bg-sidebar-accent/55 hover:text-foreground data-[active=true]:bg-sidebar-accent data-[active=true]:text-foreground data-[unread=true]:border-primary/30 data-[unread=true]:bg-primary/[0.09] data-[unread=true]:text-foreground max-[899px]:px-1.5"
                data-active={dm.id === activeId || undefined}
                data-unread={dm.unreadCount > 0 || undefined}
                onClick={() => navigate(`/dm/${dm.id}`)}
              >
                <span className="relative inline-flex shrink-0">
                  <UserAvatar user={user} size={34} />
                  <span className="absolute -right-px -bottom-px size-2.5 rounded-full border-2 border-sidebar bg-muted-foreground/60 data-[online=true]:bg-green-500 group-data-[active=true]/dmrow:border-sidebar-accent" data-online={user.online || undefined} />
                </span>
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="flex min-w-0 items-center justify-between gap-1.5 text-[13px]">
                    <strong className="truncate">{user.name}</strong>
                    {last ? <small className="shrink-0 text-[11px] text-muted-foreground/70">{relativeTime(last.createdAt)}</small> : null}
                  </span>
                  <span className="truncate text-[11px] text-muted-foreground/70">{last ? extractPreview(last.content) : `Start a conversation with ${user.name}`}</span>
                </span>
                {dm.unreadCount > 0 ? <span className="ml-auto flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-[10px] font-semibold text-primary-foreground tabular-nums">{dm.unreadCount}</span> : null}
              </button>
            )
          })}
        </div>
      </aside>
      {creating ? (
        <NewDMModal
          state={state}
          onClose={() => setCreating(false)}
          onCreated={(id) => {
            setCreating(false)
            navigate(`/dm/${id}`)
          }}
        />
      ) : null}
    </>
  )
}
