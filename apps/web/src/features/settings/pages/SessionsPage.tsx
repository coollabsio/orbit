import { Monitor, Mobile as Smartphone } from 'reicon-react'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { relativeTime } from '@/lib/format'
import { SessionRevocationError, useRevokeSessions, useSessions } from '@/features/settings/api/sessions'
import { SettingsCard } from '@/components/common/SettingsCard'
import { useSlowPending } from '@/lib/useDebouncedValue'
import { RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'

/** Account view: the current user's signed-in devices. */
export function SessionsPage() {
  const sessionsQuery = useSessions()
  const revokeSessions = useRevokeSessions()
  const revoking = useSlowPending(revokeSessions.isPending)
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
      <div className="flex flex-col divide-y">
        {sessionsQuery.isPending ? <SettingsRow>Loading sessions…</SettingsRow> : null}
        {sessionsQuery.isError ? <SettingsRow role="alert">Sessions could not be loaded. <Button variant="ghost" onClick={() => void sessionsQuery.refetch()}>Retry</Button></SettingsRow> : null}
        {sessions.map((session) => {
          return (
            <SettingsRow key={session.id} className="flex-nowrap">
              <RowIcon>
                {session.device.toLowerCase().includes('iphone') || session.device.toLowerCase().includes('android') ? <Smartphone /> : <Monitor />}
              </RowIcon>
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="flex items-center gap-2 text-[13px] font-medium text-foreground">
                  {session.device} · {session.browser}
                  {session.current ? <Badge>Current</Badge> : null}
                </span>
                <span className="text-xs text-muted-foreground/70">last active {relativeTime(session.last_activity_at)}</span>
              </div>
              <span className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
                <Avatar size="sm" className="size-7">
                  <AvatarFallback className="rounded-lg text-[11px] font-semibold">{session.user.display_name.charAt(0).toUpperCase()}</AvatarFallback>
                </Avatar>
                {session.user.display_name}
              </span>
              {!session.current ? (
                <Button type="button" variant="ghost" disabled={revokeSessions.isPending} onClick={() => revokeSessions.mutate([session.id])}>
                  Revoke
                </Button>
              ) : null}
            </SettingsRow>
          )
        })}
        {revokeSessions.isError ? <SettingsRow role="alert">{failure ? `${failure.failedIds.length} of ${failure.total} sessions could not be revoked.` : 'Session revocation failed.'} <Button variant="ghost" onClick={() => revokeSessions.mutate(failure?.failedIds ?? revokeSessions.variables ?? [])}>{failure ? 'Retry failed sessions' : 'Retry'}</Button></SettingsRow> : null}
        {revoking ? <SettingsRow role="status">Revoking {revokeSessions.variables?.length ?? 1} session{revokeSessions.variables?.length === 1 ? '' : 's'}…</SettingsRow> : null}
      </div>
    </SettingsCard>
  )
}
