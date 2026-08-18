import { Avatar } from '../../../components/ui/Avatar'
import type { AppState, User } from '../../../mock/types'

function MemberRow({ user }: { user: User }) {
  return (
    <div className="chat-member-row" data-offline={user.online ? undefined : 'true'}>
      <Avatar user={user} size={24} showOnline />
      <span className="chat-member-name" style={{ color: user.online ? user.color : undefined }}>
        {user.name}
      </span>
      <span className="chat-member-role">{user.role}</span>
    </div>
  )
}

export function MemberPanel({ state }: { state: AppState }) {
  const online = state.users.filter((u) => u.online)
  const offline = state.users.filter((u) => !u.online)
  return (
    <aside className="chat-members">
      <div className="chat-members-heading">Online — {online.length}</div>
      {online.map((user) => (
        <MemberRow key={user.id} user={user} />
      ))}
      <div className="chat-members-heading">Offline — {offline.length}</div>
      {offline.map((user) => (
        <MemberRow key={user.id} user={user} />
      ))}
    </aside>
  )
}
