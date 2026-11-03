import { View } from 'react-native'

import { ACCOUNT_NAV, Badge, Note, RowIcon, RowText, SettingsCard, SettingsRow, SettingsShell } from '../../../components/settings/Shell'
import { relativeAgo, shortDate } from '../../../data/format'
import { useRevokeSessions, useSessions } from '../../../hooks/account'
import { errorMessage } from '../../../session-context'
import { Button } from '../../../ui'

/**
 * The web's Sessions page: every signed-in device, this one marked. The server does not record the device or the
 * browser, so a row says when it signed in and when it was last active.
 */
export default function SessionsScreen() {
  const sessions = useSessions()
  const revoke = useRevokeSessions()
  const list = sessions.data ?? []
  const others = list.filter((session) => !session.current)

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV} refreshing={sessions.isRefetching} onRefresh={() => void sessions.refetch()}>
      <SettingsCard
        title="Sessions"
        description="Devices signed in to your account. Revoking a session signs that device out."
        actions={others.length > 0 ? <Button label="Sign out all other sessions" variant="outline" disabled={revoke.isPending} onPress={() => revoke.mutate(others.map((session) => session.id))} /> : undefined}
        flush
      >
        {sessions.isPending ? <View className="p-4"><Note>Loading sessions…</Note></View> : null}
        {sessions.isError ? <View className="p-4"><Note error>Sessions could not be loaded.</Note></View> : null}
        {list.map((session, index) => (
          <SettingsRow key={session.id} first={index === 0}>
            <RowIcon icon={session.current ? 'mobile' : 'monitor'} />
            <RowText
              title={session.current ? 'This device' : 'Signed-in device'}
              subtitle={`Last active ${relativeAgo(session.last_activity_at)} · signed in ${shortDate(session.created_at)}`}
              badges={session.current ? <Badge label="Current" /> : null}
            />
            {!session.current ? <Button label="Revoke" variant="ghost" size="sm" disabled={revoke.isPending} onPress={() => revoke.mutate([session.id])} /> : null}
          </SettingsRow>
        ))}
        {revoke.isPending ? <View className="p-4"><Note>Revoking {revoke.variables?.length ?? 1} session{revoke.variables?.length === 1 ? '' : 's'}…</Note></View> : null}
        {revoke.isError ? <View className="p-4"><Note error>{errorMessage(revoke.error)}</Note></View> : null}
      </SettingsCard>
    </SettingsShell>
  )
}
