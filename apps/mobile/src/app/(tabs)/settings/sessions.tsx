import { View } from 'react-native'

import { ListPager, usePagedList } from '../../../components/settings/ListPager'
import { ACCOUNT_NAV, Badge, InitialAvatar, Note, RowIcon, SettingsCard, SettingsRow, SettingsShell } from '../../../components/settings/Shell'
import { relativeTime } from '../../../data/format'
import { useRevokeSessions, useSessions } from '../../../hooks/account'
import { errorMessage } from '../../../session-context'
import { Button, Text } from '../../../ui'

/**
 * The web's Sessions page: every signed-in device, this one marked, in the web's row (name and badge, last activity,
 * the account's initial and name, Revoke). The server does not record the device or the browser.
 */
export default function SessionsScreen() {
  const sessions = useSessions()
  const revoke = useRevokeSessions()
  const list = sessions.data ?? []
  const others = list.filter((session) => !session.current)
  // The current session stays first, so it is always on page 1.
  const { visible, pager } = usePagedList([...list.filter((session) => session.current), ...others])

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV} onRefresh={() => sessions.refetch()}>
      <SettingsCard
        title="Sessions"
        description="Devices signed in to your account. Revoking a session signs that device out."
        actions={others.length > 0 ? <Button label="Sign out all other sessions" variant="outline" disabled={revoke.isPending} onPress={() => revoke.mutate(others.map((session) => session.id))} /> : undefined}
        flush
      >
        {sessions.isPending ? <View className="p-4"><Note loading>Loading sessions…</Note></View> : null}
        {sessions.isError ? <View className="p-4"><Note error>Sessions could not be loaded.</Note></View> : null}
        {visible.map((session, index) => (
          <SettingsRow key={session.id} first={index === 0}>
            <RowIcon icon={session.current ? 'mobile' : 'monitor'} />
            <View className="flex-1 min-w-0 gap-0.5">
              <View className="flex-row items-center gap-2">
                <Text className="shrink font-noto-medium text-[13px] leading-[19.5px] text-foreground">{session.current ? 'This device' : 'Signed-in device'}</Text>
                {session.current ? <Badge label="Current" /> : null}
              </View>
              <Text className="text-[12px] leading-[16px] text-muted-foreground/70">last active {relativeTime(session.last_activity_at)}</Text>
            </View>
            <View className="shrink-0 flex-row items-center gap-1.5">
              <InitialAvatar name={session.user.display_name} size={28} />
              <Text numberOfLines={1} className="max-w-24 text-[12px] leading-[16px] text-muted-foreground">{session.user.display_name}</Text>
            </View>
            {!session.current ? <Button label="Revoke" variant="ghost" disabled={revoke.isPending} onPress={() => revoke.mutate([session.id])} /> : null}
          </SettingsRow>
        ))}
        {pager.lastPage > 1 ? <ListPager {...pager} /> : null}
        {revoke.isPending ? <View className="p-4"><Note>Revoking {revoke.variables?.length ?? 1} session{revoke.variables?.length === 1 ? '' : 's'}…</Note></View> : null}
        {revoke.isError ? <View className="p-4"><Note error>{errorMessage(revoke.error)}</Note></View> : null}
      </SettingsCard>
    </SettingsShell>
  )
}
