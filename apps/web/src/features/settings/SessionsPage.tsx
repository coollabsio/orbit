import { Mobile, Monitor } from 'reicon-react'
import { relativeTime } from '../../lib/format'
import { SessionRevocationError, useRevokeSessions, useSessions } from './api/sessions'
import { SettingsCard } from './SettingsCard'

/** Admin view: every member's signed-in devices. */
export function SessionsPage() {
  const sessionsQuery = useSessions()
  const revokeSessions = useRevokeSessions()
  const sessions = sessionsQuery.data ?? []
  const others = sessions.filter((s) => !s.current)
  const failure = revokeSessions.error instanceof SessionRevocationError ? revokeSessions.error : null

  return (
    <SettingsCard
      title="Sessions"
      description="Devices signed in across the workspace. Revoking a session signs that device out."
      actions={
        others.length > 0 ? (
          <button type="button" className="button" disabled={revokeSessions.isPending} onClick={() => revokeSessions.mutate(others.map((session) => session.id))}>
            Sign out all other sessions
          </button>
        ) : undefined
      }
      flush
    >
      <div className="sessions-list">
        {sessionsQuery.isPending ? <div className="sessions-row">Loading sessions…</div> : null}
        {sessionsQuery.isError ? <div className="sessions-row" role="alert">Sessions could not be loaded. <button className="button button-ghost" onClick={() => void sessionsQuery.refetch()}>Retry</button></div> : null}
        {sessions.map((session) => {
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
                <span className="sessions-meta">last active {relativeTime(session.last_activity_at)}</span>
              </div>
              <span className="sessions-user">
                <span className="avatar-tile">{session.user.display_name.charAt(0).toUpperCase()}</span>
                {session.user.display_name}
              </span>
              {!session.current ? (
                <button type="button" className="button button-ghost" disabled={revokeSessions.isPending} onClick={() => revokeSessions.mutate([session.id])}>
                  Revoke
                </button>
              ) : null}
            </div>
          )
        })}
        {revokeSessions.isError ? <div className="sessions-row" role="alert">{failure ? `${failure.failedIds.length} of ${failure.total} sessions could not be revoked.` : 'Session revocation failed.'} <button className="button button-ghost" onClick={() => revokeSessions.mutate(failure?.failedIds ?? revokeSessions.variables ?? [])}>{failure ? 'Retry failed sessions' : 'Retry'}</button></div> : null}
        {revokeSessions.isPending ? <div className="sessions-row" role="status">Revoking {revokeSessions.variables?.length ?? 1} session{revokeSessions.variables?.length === 1 ? '' : 's'}…</div> : null}
      </div>
    </SettingsCard>
  )
}
