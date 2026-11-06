import { router, useLocalSearchParams } from 'expo-router'
import { useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import { AddPeopleSheet } from '../../../components/chat/ChannelSheets'
import { ChatSidePane, useShownPresence } from '../../../components/chat/Pane'
import { conversationPermissions, memberGroups } from '../../../data/chat'
import { presenceOf, usePresence } from '../../../data/chat-presence'
import { currentStatus } from '../../../data/status'
import * as chat from '../../../data/chat-api'
import { confirmAction, useChatWrite, useIsChatAdmin } from '../../../hooks/chat'
import { useOrbit } from '../../../orbit-context'
import { useConversations, useMembers } from '../../../queries'
import { presenceColors, useTheme } from '../../../theme'
import { Avatar, Button, EmptyState, ErrorView, Icon, IconButton, LoadingView, Text } from '../../../ui'

/**
 * The web `MembersPane`: online people under their role, offline people dimmed at the end. A tap opens the direct
 * message with them; those who manage a private channel add and remove people.
 */
export default function MembersScreen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  const { user } = useOrbit()
  const { colors } = useTheme()
  const conversations = useConversations()
  const members = useMembers()
  const admin = useIsChatAdmin()
  const run = useChatWrite()
  const [adding, setAdding] = useState(false)
  const conversation = conversations.data?.find((item) => item.id === id)
  const permissions = conversation ? conversationPermissions(conversation, user.id, admin) : null
  const shown = useShownPresence()
  const presence = usePresence()
  const groups = memberGroups(
    (conversation?.member_ids ?? []).flatMap((userId) => members.data?.find((member) => member.user_id === userId) ?? []),
    (member) => shown(member.user_id) !== 'offline',
  )

  const message = async (userId: string) => {
    const dm = await run('Could not open the conversation', (client, workspaceId) => chat.openDm(client, workspaceId, userId === user.id ? [] : [userId]))
    if (dm) router.push(`/chat/${dm.id}`)
  }
  const remove = async (userId: string, name: string) => {
    if (!conversation || !(await confirmAction(`Remove ${name}?`, `${name} leaves #${conversation.name} and needs an invitation to come back.`, 'Remove'))) return
    await run(`Could not remove ${name}`, (client, workspaceId) => chat.removeMember(client, workspaceId, conversation.id, userId))
  }

  return (
    <ChatSidePane
      title="Members"
      actions={permissions?.canAddPeople ? <IconButton icon="userAdd" label="Add people" size={32} iconSize={20} color={colors.foreground} onPress={() => setAdding(true)} /> : undefined}
    >
      {conversations.isError || members.isError ? (
        <ErrorView error={conversations.error ?? members.error} onRetry={() => void Promise.all([conversations.refetch(), members.refetch()])} />
      ) : !conversation || !members.data ? (
        <LoadingView />
      ) : groups.length === 0 ? (
        <EmptyState icon="people" title="No members to show" description="Open a conversation from the sidebar to see who is in it." />
      ) : (
        <ScrollView contentContainerClassName="px-2 pb-2">
          {groups.map((group) => (
            <View key={group.label} accessibilityLabel={`${group.label}, ${group.people.length}`}>
              <Text className="font-noto-semibold text-[12px] leading-[16px] px-2 pt-4 pb-1 text-muted-foreground">
                {group.label} — {group.people.length}
              </Text>
              <View className="gap-px">
                {group.people.map((member) => {
                  // the user's own custom status is known without the socket
                  const status = member.user_id === user.id ? currentStatus(user.status) : presenceOf(presence, member.user_id)
                  return (
                    // the web dims offline people to 40%
                    <View key={member.user_id} className={`flex-row items-center gap-1 rounded-md ${group.online ? '' : 'opacity-40'}`}>
                      <Pressable
                        accessibilityRole="button"
                        accessibilityLabel={member.user_id === user.id ? 'Message yourself' : `Message ${member.display_name}`}
                        onPress={() => void message(member.user_id)}
                        className="flex-1 min-w-0 h-[42px] flex-row items-center gap-3 px-2 rounded-md active:bg-muted"
                      >
                        <Avatar id={member.user_id} name={member.display_name} size={32} presence={shown(member.user_id)} surface={colors.card} />
                        <View className="shrink min-w-0">
                          <View className="flex-row items-center gap-1.5 min-w-0">
                            <Text numberOfLines={1} className="font-noto-medium text-[15px] leading-[19px] shrink text-muted-foreground">
                              {member.display_name}
                            </Text>
                            {/* amber-500, the same colour as the idle mark */}
                            {member.role === 'owner' ? <Icon name="crown" size={14} color={presenceColors.idle} /> : null}
                          </View>
                          {status.emoji || status.text ? (
                            <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground">
                              {[status.emoji, status.text].filter(Boolean).join(' ')}
                            </Text>
                          ) : null}
                        </View>
                      </Pressable>
                      {permissions?.canRemovePeople && member.user_id !== user.id ? <Button label="Remove" variant="ghost" size="sm" onPress={() => void remove(member.user_id, member.display_name)} /> : null}
                    </View>
                  )
                })}
              </View>
            </View>
          ))}
        </ScrollView>
      )}
      {adding && conversation ? <AddPeopleSheet conversation={conversation} onClose={() => setAdding(false)} /> : null}
    </ChatSidePane>
  )
}
