import { router } from 'expo-router'
import { useState } from 'react'

import { type ConversationAction, conversationActions, conversationPermissions } from '../../data/chat'
import * as chat from '../../data/chat-api'
import type { ConversationRecord } from '../../data/orbit'
import { confirmAction, useChatWrite, useIsChatAdmin } from '../../hooks/chat'
import { useOrbit } from '../../orbit-context'
import { useChatStates } from '../../queries'
import type { IconName } from '../../ui'
import { EditChannelSheet } from './ChannelSheets'
import { ActionSheet, type SheetItem } from './Sheet'

const NOTIFY = [
  { value: 'all', label: 'All messages' },
  { value: 'mentions', label: 'Mentions only' },
  { value: 'muted', label: 'Muted' },
] as const

/**
 * A conversation's `…` sheet: the web's `ConversationMenuItems`, for the sidebar row or the conversation header. It
 * owns the sheets it opens (notifications, Edit channel).
 */
export function ConversationOptions({ conversation, title, placement, visible, onClose }: { conversation: ConversationRecord; title: string; placement: 'row' | 'header'; visible: boolean; onClose: () => void }) {
  const { user } = useOrbit()
  const admin = useIsChatAdmin()
  const state = useChatStates().data?.find((item) => item.conversation_id === conversation.id)
  const run = useChatWrite()
  const [next, setNext] = useState<'notify' | 'edit' | null>(null)
  const id = conversation.id
  const permissions = conversationPermissions(conversation, user.id, admin)
  const notifyLabel = NOTIFY.find((option) => option.value === state?.notify)?.label

  const leave = async () => {
    // leaving a private channel removes it for the user: they need an invitation to come back
    const confirmed = await confirmAction(`Leave ${conversation.name}?`, 'This is a private channel. To come back, a member has to add you again.', 'Leave channel')
    if (!confirmed) return
    if ((await run('Could not leave the channel', (client, workspaceId) => chat.leaveChannel(client, workspaceId, id))) && placement === 'header') router.navigate('/chat')
  }
  const archive = async () => {
    const confirmed = await confirmAction(`Archive #${conversation.name}?`, 'The channel becomes read-only and leaves the sidebar for everyone. Its messages stay searchable.', 'Archive channel')
    if (!confirmed) return
    if ((await run('Could not archive the channel', (client, workspaceId) => chat.archiveChannel(client, workspaceId, id))) && placement === 'header') router.navigate('/chat')
  }

  const item = (key: ConversationAction): SheetItem => {
    const entry = (label: string, icon: IconName, onPress: () => void, destructive = false): SheetItem => ({ key, label, icon, onPress, destructive })
    switch (key) {
      case 'mark-read':
        return entry('Mark as read', 'checkCircle', () => void run('Could not mark as read', (client, workspaceId) => chat.markRead(client, workspaceId, id)))
      case 'favorite':
        return entry(state?.favorite ? 'Remove star' : 'Star', 'star', () =>
          void run(state?.favorite ? 'Could not remove the star' : 'Could not add the star', (client, workspaceId) => chat.setConversationState(client, workspaceId, id, { favorite: !state?.favorite })),
        )
      case 'notify':
        return entry(notifyLabel ? `Notifications (${notifyLabel})` : 'Notifications', 'bell', () => setNext('notify'))
      case 'threads':
        return entry('Threads', 'layer', () => router.push(`/chat/${id}/threads`))
      case 'pins':
        return entry('Pinned messages', 'pinTack', () => router.push(`/chat/${id}/pins`))
      case 'files':
        return entry('Files', 'attach', () => router.push(`/chat/${id}/files`))
      case 'members':
        return entry('Members', 'people', () => router.push(`/chat/${id}/members`))
      case 'edit':
        return entry('Edit channel', 'edit', () => setNext('edit'))
      case 'leave':
        return entry('Leave channel', 'logout', () => void leave())
      case 'archive':
        return entry('Archive channel', 'archive', () => void archive(), true)
    }
  }

  return (
    <>
      <ActionSheet visible={visible} title={title} items={conversationActions(placement, state, permissions).map(item)} onClose={onClose} />
      <ActionSheet
        visible={next === 'notify'}
        title="Notifications"
        items={NOTIFY.map((option) => ({
          key: option.value,
          label: option.label,
          selected: state?.notify === option.value,
          onPress: () => void run('Could not change the notification setting', (client, workspaceId) => chat.setConversationState(client, workspaceId, id, { notify: option.value })),
        }))}
        onClose={() => setNext(null)}
      />
      {next === 'edit' ? <EditChannelSheet conversation={conversation} onClose={() => setNext(null)} /> : null}
    </>
  )
}
