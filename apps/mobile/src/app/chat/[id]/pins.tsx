import { useLocalSearchParams } from 'expo-router'

import { ConversationListScreen } from '../../../components/chat/ConversationListScreen'

export default function Screen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  return <ConversationListScreen conversationId={id} kind="pins" />
}
