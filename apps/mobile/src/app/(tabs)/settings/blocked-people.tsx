import { View } from 'react-native'

import { ACCOUNT_NAV, Note, SettingsCard, SettingsRow, SettingsShell } from '../../../components/settings/Shell'
import { relativeTime } from '../../../data/format'
import { useBlocks, useSetBlocked } from '../../../hooks/chat'
import { Avatar, Button, Text } from '../../../ui'

/** The people you blocked in chat, in every workspace, each with Unblock. */
export default function BlockedPeopleScreen() {
  const blocks = useBlocks()
  const setBlocked = useSetBlocked()
  const list = blocks.data ?? []

  return (
    <SettingsShell title="Account settings" nav={ACCOUNT_NAV} onRefresh={() => blocks.refetch()}>
      <SettingsCard title="Blocked people" description="Messages from these people are hidden from you in chat, and they cannot send you direct messages." flush>
        {blocks.isPending ? <View className="p-4"><Note loading>Loading blocked people…</Note></View> : null}
        {blocks.isError ? <View className="p-4"><Note error>Blocked people could not be loaded.</Note></View> : null}
        {blocks.isSuccess && list.length === 0 ? <View className="p-4"><Note>You did not block anyone.</Note></View> : null}
        {list.map((block, index) => (
          <SettingsRow key={block.user_id} first={index === 0}>
            <Avatar id={block.user_id} name={block.display_name} url={block.avatar_url ?? null} size={32} />
            <View className="flex-1 min-w-0 gap-0.5">
              <Text numberOfLines={1} className="font-noto-medium text-[13px] leading-[19.5px] text-foreground">{block.display_name}</Text>
              <Text className="text-[12px] leading-[16px] text-muted-foreground/70">blocked {relativeTime(block.created_at)}</Text>
            </View>
            <Button label="Unblock" variant="outline" size="sm" onPress={() => void setBlocked({ id: block.user_id, name: block.display_name }, false)} />
          </SettingsRow>
        ))}
      </SettingsCard>
    </SettingsShell>
  )
}
