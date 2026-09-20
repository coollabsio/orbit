import { Monitor, Smartphone } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { relativeTime } from '@/lib/format'
import { SessionRevocationError, useRevokeSessions, useSessions } from '@/features/settings/api/sessions'
import { SettingsCard } from '@/components/common/SettingsCard'

/** Account view: the current user's signed-in devices. */
export function SessionsPage() {
  const sessionsQuery = useSessions()
  const revokeSessions = useRevokeSessions()
  const sessions = sessionsQuery.data ?? []
  const others = sessions.filter((s) => !s.current)
  const failure = revokeSessions.error instanceof SessionRevocationError ? revokeSessions.error : null

  return (
    <SettingsCard
      title="Sessions"
      description="Devices signed in to your account. Revoking a session signs that device out."
      actions={
        others.length > 0 ? (
          <Button type="button" variant="outline" disabled={revokeSessions.isPending} onClick={() => revokeSessions.mutate(others.map((session) => session.id))}>
            Sign out all other sessions
          </Button>
        ) : undefined
      }
      flush
    >
      <div className="flex flex-col divide-y divide-border">
        {sessionsQuery.isPending ? <div className="flex items-center gap-3 px-4 py-3">Loading sessions…</div> : null}
        {sessionsQuery.isError ? <div className="flex items-center gap-3 px-4 py-3" role="alert">Sessions could not be loaded. <Button variant="ghost" onClick={() => void sessionsQuery.refetch()}>Retry</Button></div> : null}
        {sessions.map((session) => {
          return (
            <div key={session.id} className="flex items-center gap-3 px-4 py-3">
              <span className="inline-flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                {session.device.toLowerCase().includes('iphone') || session.device.toLowerCase().includes('android') ? (
                  <Smartphone className="size-4.5" />
                ) : (
                  <Monitor className="size-4.5" />
                )}
              </span>
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="flex items-center gap-2 text-[13px] font-medium text-foreground">
                  {session.device} · {session.browser}
                  {session.current ? <Badge className="h-auto rounded-full border-0 bg-primary/10 text-[10px] leading-[14px] text-primary">Current</Badge> : null}
                </span>
                <span className="text-xs text-muted-foreground/70">last active {relativeTime(session.last_activity_at)}</span>
              </div>
              <span className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
                <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-lg bg-sidebar-accent text-[11px] font-semibold text-muted-foreground uppercase">{session.user.display_name.charAt(0).toUpperCase()}</span>
                {session.user.display_name}
              </span>
              {!session.current ? (
                <Button type="button" variant="ghost" disabled={revokeSessions.isPending} onClick={() => revokeSessions.mutate([session.id])}>
                  Revoke
                </Button>
              ) : null}
            </div>
          )
        })}
        {revokeSessions.isError ? <div className="flex items-center gap-3 px-4 py-3" role="alert">{failure ? `${failure.failedIds.length} of ${failure.total} sessions could not be revoked.` : 'Session revocation failed.'} <Button variant="ghost" onClick={() => revokeSessions.mutate(failure?.failedIds ?? revokeSessions.variables ?? [])}>{failure ? 'Retry failed sessions' : 'Retry'}</Button></div> : null}
        {revokeSessions.isPending ? <div className="flex items-center gap-3 px-4 py-3" role="status">Revoking {revokeSessions.variables?.length ?? 1} session{revokeSessions.variables?.length === 1 ? '' : 's'}…</div> : null}
      </div>
    </SettingsCard>
  )
}
