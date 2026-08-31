import { Mobile, Monitor } from 'reicon-react'
import { Avatar } from '../../components/ui/Avatar'
import { revokeOtherSessions, revokeSession } from '../../mock/actions'
import { useAppState } from '../../mock/store'
import { relativeTime } from '../../lib/format'
import { SettingsCard } from './SettingsCard'

/** Admin view: every member's signed-in devices. */
export function SessionsPage() {
  const { sessions, users } = useAppState()
  const others = sessions.filter((s) => !s.current)

  return (
    <SettingsCard
      title="Sessions"
      description="Devices signed in across the workspace. Revoking a session signs that device out."
      actions={
        others.length > 0 ? (
          <button type="button" className="button" onClick={revokeOtherSessions}>
            Sign out all other sessions
          </button>
        ) : undefined
      }
      flush
    >
      <div className="sessions-list">
        {sessions.map((session) => {
          const user = users.find((u) => u.id === session.userId)
          return (
            <div key={session.id} className="sessions-row">
              <span className="sessions-device-icon">
                {session.device.toLowerCase().includes('iphone') || session.device.toLowerCase().includes('android') ? (
                  <Mobile size={18} />
                ) : (
                  <Monitor size={18} />
                )}
              </span>
              <div className="sessions-text">
                <span className="sessions-device">
                  {session.device} · {session.browser}
                  {session.current ? <span className="badge sessions-current">Current</span> : null}
                </span>
                <span className="sessions-meta">last active {relativeTime(session.lastActiveAt)}</span>
              </div>
              <span className="sessions-user">
                <Avatar user={user} size={18} />
                {user?.name ?? 'Unknown'}
              </span>
              {!session.current ? (
                <button type="button" className="button button-ghost" onClick={() => revokeSession(session.id)}>
                  Revoke
                </button>
              ) : null}
            </div>
          )
        })}
      </div>
    </SettingsCard>
  )
}
