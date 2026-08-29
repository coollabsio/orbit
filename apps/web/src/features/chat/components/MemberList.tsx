// Port of the chat reference MemberList: role groups, Online/Offline sections, presence dots, resizable.
import { useEffect, useState } from 'react'
import { Xmark } from 'reicon-react'
import type { AppState, User } from '../../../mock/types'
import { primaryRole } from '../chatLib'

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
    <div className="fc-members" style={isMobile ? { height: '100%', width: 240 } : { width }}>
      {!isMobile ? (
        <div className="fc-members-resize" onPointerDown={handleResizeStart} title="Resize member list" />
      ) : null}
      {isMobile ? (
        <div className="fc-members-mobile-header">
          <span>Members</span>
          <button type="button" className="fc-composer-reply-close" title="Close" onClick={onClose}>
            <Xmark />
          </button>
        </div>
      ) : null}
      <div className="fc-members-scroll">
        {roleGroups.map(({ role, members }) => (
          <div key={role.id} className="fc-members-section">
            <div className="fc-members-heading">
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
          <div className="fc-members-section">
            <div className="fc-members-heading">
              <span>Online — {online.length}</span>
            </div>
            {online.map((member) => (
              <MemberItem key={member.id} member={member} online nameColor={colorOf(member)} />
            ))}
          </div>
        ) : null}
        {offline.length > 0 ? (
          <div className="fc-members-section">
            <div className="fc-members-heading">
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
        <div className="fc-members-mobile-backdrop" onClick={onClose} />
        <div className="fc-members-mobile">{content}</div>
      </>
    )
  }

  return <div className="fc-members-desktop" style={{ display: 'flex' }}>{content}</div>
}

function MemberItem({ member, online, nameColor }: { member: User; online: boolean; nameColor?: string }) {
  return (
    <div className="fc-member-item">
      <div className="fc-member-avatar-wrap">
        <div
          className="fc-member-avatar"
          style={{ background: `color-mix(in srgb, ${member.color} 22%, transparent)`, color: member.color }}
        >
          {member.name.charAt(0).toUpperCase()}
        </div>
        {online ? <span className="fc-presence" /> : null}
      </div>
      <span
        className="fc-member-name"
        data-online={online ? 'true' : 'false'}
        style={nameColor ? { color: nameColor, opacity: online ? 1 : 0.65 } : undefined}
      >
        {member.name}
      </span>
    </div>
  )
}
