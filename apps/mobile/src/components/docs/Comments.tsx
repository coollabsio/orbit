import { useState } from 'react'
import { KeyboardAvoidingView, Modal, Platform, ScrollView, View } from 'react-native'
import { TextInput } from '../../ui/scaled'

import type { PageThread } from '../../data/docs-api'
import { visibleThreads } from '../../data/docs'
import { relativeAgo } from '../../data/format'
import { useAddComment, usePageThreads } from '../../hooks/docs'
import { usePeople } from '../../queries'
import { errorMessage } from '../../session-context'
import { alpha, useTheme } from '../../theme'
import { Avatar, Button, IconButton, LoadingView, PaneHeader, SafeAreaView, Text } from '../../ui'

/**
 * The page's comments panel at phone size: every thread with its comments, a reply field under each, and a new
 * comment on the whole page at the bottom. Comments on a passage start on the web, where the text can be selected.
 */
export function CommentsSheet({ pageId, visible, onClose }: { pageId: string; visible: boolean; onClose: () => void }) {
  const { colors } = useTheme()
  const threads = usePageThreads(pageId)
  const add = useAddComment(pageId)
  const [text, setText] = useState('')
  const list = visibleThreads(threads.data ?? [])
  const submit = () => {
    if (!text.trim()) return
    add.mutate({ text }, { onSuccess: () => setText('') })
  }
  return (
    <Modal visible={visible} animationType="slide" presentationStyle="pageSheet" onRequestClose={onClose}>
      <SafeAreaView edges={['top', 'bottom', 'left', 'right']} className="flex-1 bg-background">
        <PaneHeader className="border-b border-border">
          <Text className="font-noto-medium text-[13.5px] flex-1 pl-1">Comments</Text>
          <IconButton icon="close" label="Close comments" color={alpha(colors.mutedForeground, 0.7)} onPress={onClose} />
        </PaneHeader>
        <KeyboardAvoidingView className="flex-1" behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
          {!threads.data ? (
            threads.isError ? (
              <View className="p-4">
                <Text className="text-[13px] text-destructive">Comments could not be loaded.</Text>
              </View>
            ) : (
              <LoadingView />
            )
          ) : (
            <ScrollView className="flex-1" contentContainerClassName="p-3 gap-3" keyboardShouldPersistTaps="handled">
              {list.length === 0 ? <Text className="text-[13px] p-1 text-muted-foreground">No comments yet.</Text> : null}
              {list.map((thread) => (
                <ThreadCard key={thread.id} pageId={pageId} thread={thread} />
              ))}
            </ScrollView>
          )}
          <View className="flex-row items-end gap-2 p-3 border-t border-border">
            <TextInput
              accessibilityLabel="Comment on this page"
              placeholder="Comment on this page"
              placeholderTextColor={colors.mutedForeground}
              value={text}
              onChangeText={setText}
              multiline
              className="font-noto text-[15px] leading-[1.45] flex-1 max-h-30 min-h-9 px-2.5 py-[7px] rounded-lg border border-input text-foreground"
            />
            <Button label="Comment" loading={add.isPending} disabled={!text.trim()} onPress={submit} />
          </View>
          {add.isError ? <Text className="text-[12px] px-3 pb-2 text-destructive">{errorMessage(add.error)}</Text> : null}
        </KeyboardAvoidingView>
      </SafeAreaView>
    </Modal>
  )
}

function ThreadCard({ pageId, thread }: { pageId: string; thread: PageThread }) {
  const { colors } = useTheme()
  const people = usePeople()
  const reply = useAddComment(pageId)
  const [text, setText] = useState('')
  const nameOf = (id: string) => people.find((person) => person.id === id)?.name ?? 'Someone'
  const comments = thread.comments.filter((comment) => !comment.deleted_at)
  return (
    <View className={`rounded-lg border border-border p-3 gap-2.5 ${thread.resolved ? 'opacity-75' : 'opacity-100'}`}>
      {thread.quote.trim() ? (
        <Text numberOfLines={2} className="text-[12px] pl-2 border-l-2 border-primary text-muted-foreground">
          {thread.quote.trim()}
        </Text>
      ) : null}
      {thread.resolved ? <Text className="font-noto-medium text-[11px] text-muted-foreground">Resolved</Text> : null}
      {comments.map((comment) => (
        <View key={comment.id} className="flex-row gap-2">
          <Avatar id={comment.author_id} name={nameOf(comment.author_id)} size={24} />
          <View className="flex-1 gap-0.5">
            <Text className="font-noto-medium text-[13px]">
              {nameOf(comment.author_id)} <Text className="text-[12px] text-muted-foreground">{relativeAgo(comment.created_at)}</Text>
            </Text>
            <Text selectable className="text-[14px] leading-[20px]">
              {comment.body_text.trim()}
            </Text>
          </View>
        </View>
      ))}
      <View className="flex-row items-center gap-2">
        <TextInput
          accessibilityLabel="Reply"
          placeholder="Reply…"
          placeholderTextColor={colors.mutedForeground}
          value={text}
          onChangeText={setText}
          className="font-noto text-[14px] leading-[1.45] flex-1 h-8 px-2.5 py-0 rounded-md bg-muted text-foreground"
        />
        {text.trim() ? <Button label="Reply" size="sm" loading={reply.isPending} onPress={() => reply.mutate({ text, threadId: thread.id }, { onSuccess: () => setText('') })} /> : null}
      </View>
      {reply.isError ? <Text className="text-[12px] text-destructive">{errorMessage(reply.error)}</Text> : null}
    </View>
  )
}
