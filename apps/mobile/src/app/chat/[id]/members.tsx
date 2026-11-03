import { router, useLocalSearchParams } from 'expo-router'
import { useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import { AddPeopleSheet } from '../../../components/chat/ChannelSheets'
import { ChatPane } from '../../../components/chat/Pane'
import { conversationPermissions, conversationTitle } from '../../../data/chat'
import * as chat from '../../../data/chat-api'
import { confirmAction, useChatWrite, useIsChatAdmin } from '../../../hooks/chat'
import { useOrbit } from '../../../orbit-context'
import { useConversations, useMembers, usePeople } from '../../../queries'
import { presenceColors, useTheme } from '../../../theme'
import { Avatar, Button, ErrorView, Icon, IconButton, LoadingView, Text } from '../../../ui'
import { ChatColorScope } from '../../../components/chat/ChatColorScope'

/**
 * The people in a conversation, by name. A tap opens the direct message with them; those who manage a private channel
 * add and remove people.
 */
export default function MembersScreen() {
  return (
    <ChatColorScope surface="page">
      <MembersScreenContent />
    </ChatColorScope>
  )
}

function MembersScreenContent() {
  const { id } = useLocalSearchParams<{ id: string }>()
  const { user } = useOrbit()
  const { colors } = useTheme()
  const conversations = useConversations()
  const members = useMembers()
  const people = usePeople()
  const admin = useIsChatAdmin()
  const run = useChatWrite()
  const [adding, setAdding] = useState(false)
  const conversation = conversations.data?.find((item) => item.id === id)
  const permissions = conversation ? conversationPermissions(conversation, user.id, admin) : null
  const inIt = (conversation?.member_ids ?? [])
    .flatMap((userId) => members.data?.find((member) => member.user_id === userId) ?? [])
    .sort((a, b) => a.display_name.localeCompare(b.display_name))

  const message = async (userId: string) => {
    const dm = await run('Could not open the conversation', (client, workspaceId) => chat.openDm(client, workspaceId, userId === user.id ? [] : [userId]))
    if (dm) router.push(`/chat/${dm.id}`)
  }
  const remove = async (userId: string, name: string) => {
    if (!conversation || !(await confirmAction(`Remove ${name}?`, `${name} leaves #${conversation.name} and needs an invitation to come back.`, 'Remove'))) return
    await run(`Could not remove ${name}`, (client, workspaceId) => chat.removeMember(client, workspaceId, conversation.id, userId))
  }

  return (
    <ChatPane
      title="Members"
      subtitle={conversation ? (conversation.kind === 'dm' ? conversationTitle(conversation, people, user.id) : `#${conversation.name}`) : undefined}
      actions={permissions?.canAddPeople ? <IconButton icon="userAdd" label="Add people" size={32} iconSize={20} color={colors.mutedForeground} onPress={() => setAdding(true)} /> : undefined}
    >
      {conversations.isError || members.isError ? (
        <ErrorView error={conversations.error ?? members.error} onRetry={() => void Promise.all([conversations.refetch(), members.refetch()])} />
      ) : !conversation || !members.data ? (
        <LoadingView />
      ) : (
        <ScrollView contentContainerClassName="px-2 pb-4">
          <Text className="font-noto-semibold text-[12px] px-2 pt-2 pb-1 text-muted-foreground">Members, {inIt.length}</Text>
          {inIt.map((member) => (
            <View key={member.user_id} className="flex-row items-center gap-1">
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={member.user_id === user.id ? 'Message yourself' : `Message ${member.display_name}`}
                onPress={() => void message(member.user_id)}
                className="flex-1 h-12 flex-row items-center gap-3 px-2 rounded-md active:bg-muted/80"
              >
                <Avatar id={member.user_id} name={member.display_name} size={32} />
                <Text numberOfLines={1} className="font-noto-medium text-[15px] shrink">
                  {member.display_name}
                  {member.user_id === user.id ? ' (you)' : ''}
                </Text>
                {/* the web's amber-500 crown, the same colour as the idle mark */}
                {member.role === 'owner' ? <Icon name="crown" size={14} color={presenceColors.idle} /> : null}
              </Pressable>
              {permissions?.canRemovePeople && member.user_id !== user.id ? <Button label="Remove" variant="ghost" size="sm" onPress={() => void remove(member.user_id, member.display_name)} /> : null}
            </View>
          ))}
        </ScrollView>
      )}
      {adding && conversation ? <AddPeopleSheet conversation={conversation} onClose={() => setAdding(false)} /> : null}
    </ChatPane>
  )
}
