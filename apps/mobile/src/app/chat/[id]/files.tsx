import { useLocalSearchParams } from 'expo-router'

import { ChatColorScope } from '../../../components/chat/ChatColorScope'
import { ConversationListScreen } from '../../../components/chat/ConversationListScreen'

export default function Screen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  return (
    <ChatColorScope surface="page">
      <ConversationListScreen conversationId={id} kind="files" />
    </ChatColorScope>
  )
}
