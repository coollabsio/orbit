// Port of the chat reference MemberList: role groups, Online/Offline sections, presence dots, resizable.
import { useEffect, useState } from 'react'
import { X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import type { AppState, User } from '@/mock/types'
import { primaryRole } from '@/features/chat/chatLib'

const WIDTH_KEY = 'orbit:member_list_width'
const MIN_WIDTH = 220
const MAX_WIDTH = 420
const DEFAULT_WIDTH = 240

function clampWidth(width: number) {
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, width))
}

function storedWidth(): number {
  const value = Number(window.localStorage.getItem(WIDTH_KEY))
  return Number.isFinite(value) && value > 0 ? clampWidth(value) : DEFAULT_WIDTH
}

export function MemberList({
  state,
  isMobile = false,
  onClose,
}: {
  state: AppState
  isMobile?: boolean
  onClose?: () => void
}) {
  const [width, setWidth] = useState(storedWidth)

  useEffect(() => {
    if (!isMobile) window.localStorage.setItem(WIDTH_KEY, String(width))
  }, [isMobile, width])

  function handleResizeStart(event: React.PointerEvent<HTMLDivElement>) {
    if (isMobile) return
    event.preventDefault()
    function onMove(moveEvent: PointerEvent) {
      setWidth(clampWidth(window.innerWidth - moveEvent.clientX))
    }
    function onUp() {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  // online members with a primary role are grouped under that role; the rest go to Online / Offline
  const roles = [...state.roles].sort((a, b) => a.position - b.position)
  const buckets = new Map<string, User[]>()
  const online: User[] = []
  const offline: User[] = []
  for (const member of state.users) {
    const role = member.online ? primaryRole(state, member.id) : undefined
    if (role) buckets.set(role.id, [...(buckets.get(role.id) ?? []), member])
    else if (member.online) online.push(member)
    else offline.push(member)
  }
  const roleGroups = roles.map((role) => ({ role, members: buckets.get(role.id) ?? [] })).filter((group) => group.members.length > 0)
  const colorOf = (member: User) => primaryRole(state, member.id)?.color

  const content = (
    <div className={`relative flex shrink-0 flex-col border-l border-border bg-background text-foreground ${isMobile ? 'h-full w-60' : ''}`} style={isMobile ? undefined : { width }}>
      {!isMobile ? (
        <div className="absolute top-0 bottom-0 -left-px z-20 w-1 cursor-col-resize transition-colors hover:bg-primary/40" onPointerDown={handleResizeStart} title="Resize member list" />
      ) : null}
      {isMobile ? (
        <div className="flex h-14 shrink-0 items-center justify-between border-b border-border px-4">
          <span className="text-sm font-semibold">Members</span>
          <Button type="button" variant="ghost" size="icon-sm" className="grid size-7 shrink-0 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground dark:hover:bg-muted" title="Close" onClick={onClose}>
            <X className="size-3.5" />
          </Button>
        </div>
      ) : null}
      <div className="flex min-h-0 flex-1 flex-col overflow-auto [overscroll-behavior:none]">
        {roleGroups.map(({ role, members }) => (
          <div key={role.id} className="flex min-w-0 flex-col gap-0.5 px-4 pt-4">
            <div className="mb-0.5 flex items-center gap-2 pl-0.5 text-xs leading-5 font-semibold text-muted-foreground">
              <span>
                {role.name} — {members.length}
              </span>
            </div>
            {members.map((member) => (
              <MemberItem key={member.id} member={member} online nameColor={colorOf(member)} />
            ))}
          </div>
        ))}
        {online.length > 0 ? (
          <div className="flex min-w-0 flex-col gap-0.5 px-4 pt-4">
            <div className="mb-0.5 flex items-center gap-2 pl-0.5 text-xs leading-5 font-semibold text-muted-foreground">
              <span>Online — {online.length}</span>
            </div>
            {online.map((member) => (
              <MemberItem key={member.id} member={member} online nameColor={colorOf(member)} />
            ))}
          </div>
        ) : null}
        {offline.length > 0 ? (
          <div className="flex min-w-0 flex-col gap-0.5 px-4 pt-4">
            <div className="mb-0.5 flex items-center gap-2 pl-0.5 text-xs leading-5 font-semibold text-muted-foreground">
              <span>Offline — {offline.length}</span>
            </div>
            {offline.map((member) => (
              <MemberItem key={member.id} member={member} online={false} nameColor={colorOf(member)} />
            ))}
          </div>
        ) : null}
      </div>
    </div>
  )

  if (isMobile) {
    return (
      <>
        <div className="fixed inset-0 z-40 bg-black/50" onClick={onClose} />
        <div className="fixed top-0 right-0 bottom-0 z-50 duration-200 animate-in slide-in-from-right motion-reduce:animate-none">{content}</div>
      </>
    )
  }

  return <div className="flex max-[1279px]:hidden">{content}</div>
}

function MemberItem({ member, online, nameColor }: { member: User; online: boolean; nameColor?: string }) {
  return (
    <div className="flex min-w-0 cursor-pointer items-center gap-2 rounded-lg p-2 text-sm leading-5 font-medium transition-colors hover:bg-muted">
      <div className="relative shrink-0">
        <div
          className="flex size-6 items-center justify-center overflow-hidden rounded-full bg-muted text-xs font-medium text-muted-foreground"
          style={{ background: `color-mix(in srgb, ${member.color} 22%, transparent)`, color: member.color }}
        >
          {member.name.charAt(0).toUpperCase()}
        </div>
        {online ? <span className="absolute -right-0.5 -bottom-0.5 flex size-3 rounded-full border-2 border-background bg-green-500" /> : null}
      </div>
      <span
        className={`min-w-0 truncate text-foreground data-[online=false]:text-muted-foreground ${nameColor && !online ? 'opacity-65' : ''}`}
        data-online={online ? 'true' : 'false'}
        style={nameColor ? { color: nameColor } : undefined}
      >
        {member.name}
      </span>
    </div>
  )
}
