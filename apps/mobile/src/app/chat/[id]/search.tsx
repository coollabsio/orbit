import { router, useLocalSearchParams } from 'expo-router'
import { useEffect, useState } from 'react'
import { FlatList, Pressable, View } from 'react-native'
import { TextInput } from '../../../ui/scaled'

import { useMessageText } from '../../../components/chat/MessageList'
import { ChatSidePane, MessageByline } from '../../../components/chat/Pane'
import { conversationTitle, decodeMentions, leadToFirstMatch, type Person, splitByRanges } from '../../../data/chat'
import type { MessageRecord } from '../../../data/orbit'
import { useChatSearch } from '../../../hooks/chat'
import { useOrbit } from '../../../orbit-context'
import { useConversations, usePeople } from '../../../queries'
import { useTheme } from '../../../theme'
import { Button, EmptyState, ErrorView, Icon, LoadingView, PickerSheet, Spinner, Text } from '../../../ui'
import { ChatColorScope } from '../../../components/chat/ChatColorScope'

/** Search in a conversation (or all of them) with the web's filters. A result opens the message where it is. */
export default function SearchScreen() {
  return (
    <ChatColorScope surface="page">
      <SearchScreenContent />
    </ChatColorScope>
  )
}

function SearchScreenContent() {
  const { id, q } = useLocalSearchParams<{ id: string; q?: string }>()
  const { user } = useOrbit()
  const { colors, scheme } = useTheme()
  const people = usePeople()
  const conversations = useConversations().data ?? []
  const words = useMessageText()
  const [input, setInput] = useState(q ?? '')
  const [query, setQuery] = useState((q ?? '').trim())
  const [thisConversation, setThisConversation] = useState(true)
  const [authorId, setAuthorId] = useState<string | null>(null)
  const [hasFile, setHasFile] = useState(false)
  const [pickAuthor, setPickAuthor] = useState(false)
  const conversation = conversations.find((item) => item.id === id)
  const title = conversation ? conversationTitle(conversation, people, user.id) : ''

  // search as the user types, a moment after the last key
  useEffect(() => {
    const timer = setTimeout(() => setQuery(input.trim()), 300)
    return () => clearTimeout(timer)
  }, [input])

  const search = useChatSearch({ query, conversationId: thisConversation ? id : undefined, authorId: authorId ?? undefined, hasFile: hasFile || undefined })
  const searching = query !== '' || authorId !== null || hasFile
  const hits = search.data?.pages.flatMap((page) => page.items) ?? []
  const channels: Person[] = conversations.filter((item) => item.kind !== 'dm').map((item) => ({ id: item.id, name: item.name }))

  const open = (message: MessageRecord) => {
    if (message.thread_root_id && !message.also_in_channel) return router.push(`/chat/thread/${message.thread_root_id}`)
    if (message.conversation_id === id) return router.dismissTo(`/chat/${id}?message=${message.id}`)
    router.push(`/chat/${message.conversation_id}?message=${message.id}`)
  }

  // the web's outline `Toggle` at `sm`: muted when pressed
  const toggle = (label: string, on: boolean, onPress: () => void) => (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected: on }}
      onPress={onPress}
      className={`h-7 flex-row items-center px-2.5 rounded-md border border-input ${on ? 'bg-muted' : 'active:bg-muted'}`}
    >
      <Text className="font-noto-medium text-[12.8px] leading-[18px]">{label}</Text>
    </Pressable>
  )

  return (
    <ChatSidePane title="Search">
      {/* the web's search field sits in the conversation header, behind its sheet; here it leads the filters */}
      <View className="p-2 gap-2 border-b border-border">
        <View className="h-9 flex-row items-center gap-2 px-2.5 rounded-lg bg-secondary/60">
          <Icon name="search" size={16} color={colors.mutedForeground} />
          <TextInput
            autoFocus
            value={input}
            onChangeText={setInput}
            onSubmitEditing={() => setQuery(input.trim())}
            returnKeyType="search"
            placeholder="Search"
            accessibilityLabel={`Search in ${title}`}
            placeholderTextColor={colors.mutedForeground}
            selectionColor={colors.primary}
            keyboardAppearance={scheme}
            autoCapitalize="none"
            autoCorrect={false}
            className="font-noto text-[16px] leading-[1.45] flex-1 py-0 text-foreground"
          />
        </View>
        <View accessibilityLabel="Filters" className="flex-row flex-wrap items-center gap-1.5">
          {toggle('This conversation', thisConversation, () => setThisConversation(!thisConversation))}
          {/* the web's `Select` trigger at `sm` */}
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="From a member"
            onPress={() => setPickAuthor(true)}
            className={`h-7 max-w-44 flex-row items-center gap-1.5 pl-2.5 pr-2 rounded-md border border-input ${scheme === 'dark' ? 'bg-input/30 active:bg-input/50' : 'active:bg-muted'}`}
          >
            <Text numberOfLines={1} className="shrink text-[14px] leading-[20px]">
              {authorId ? `From ${words.nameOf(authorId)}` : 'From…'}
            </Text>
            <Icon name="chevronDown" size={16} color={colors.mutedForeground} />
          </Pressable>
          {toggle('Has file', hasFile, () => setHasFile(!hasFile))}
        </View>
      </View>
      {!searching ? (
        <EmptyState icon="search" title="Search messages" description="Type in the search field, or choose a filter." />
      ) : search.isPending ? (
        <LoadingView />
      ) : search.isError ? (
        <ErrorView error={search.error} onRetry={() => void search.refetch()} />
      ) : hits.length === 0 ? (
        <EmptyState
          icon="search"
          title={query ? `No results for "${query}"` : 'No results'}
          description={thisConversation ? 'Nothing matches in this conversation.' : 'Try other words or remove a filter.'}
          action={thisConversation ? <Button label="Search all conversations" variant="outline" size="sm" onPress={() => setThisConversation(false)} /> : undefined}
        />
      ) : (
        <FlatList
          data={hits}
          keyExtractor={(hit) => hit.message.id}
          keyboardShouldPersistTaps="handled"
          contentContainerClassName="p-2 gap-1"
          onEndReached={() => {
            if (search.hasNextPage && !search.isFetchingNextPage) void search.fetchNextPage()
          }}
          ListFooterComponent={search.isFetchingNextPage ? <View className="p-4"><Spinner /></View> : undefined}
          renderItem={({ item: hit }) => {
            const where = conversations.find((item) => item.id === hit.conversation_id)
            const segments = leadToFirstMatch(splitByRanges(hit.message.body, hit.ranges)).map((segment) => ({ ...segment, text: decodeMentions(segment.text, people, channels) }))
            const files = hit.message.attachments.length
            return (
              // the web `SearchResult`: the whole row is the button
              <Pressable accessibilityRole="button" onPress={() => open(hit.message)} className="gap-1 p-2 rounded-lg active:bg-muted">
                <MessageByline message={hit.message} author={words.nameOf(hit.message.author_id)}>
                  {/* a result names its conversation only when the search covers all of them */}
                  {!thisConversation && where ? (
                    <Text numberOfLines={1} className="shrink text-[12px] leading-[16px] text-muted-foreground">
                      in {where.kind === 'dm' ? '' : '#'}
                      {conversationTitle(where, people, user.id)}
                    </Text>
                  ) : null}
                </MessageByline>
                {hit.message.body ? (
                  <Text numberOfLines={4} className="text-[14px] leading-[20px]">
                    {segments.map((segment, index) =>
                      segment.match ? (
                        <Text key={index} className="text-[14px] leading-[20px] rounded-xs bg-primary/20">
                          {segment.text}
                        </Text>
                      ) : (
                        segment.text
                      ),
                    )}
                  </Text>
                ) : null}
                {files > 0 ? (
                  <View className="flex-row items-center gap-1">
                    <Icon name="attach" size={14} color={colors.mutedForeground} />
                    <Text numberOfLines={1} className="shrink text-[12px] leading-[16px] text-muted-foreground">
                      {files === 1 ? hit.message.attachments[0]!.file_name : `${files} files`}
                    </Text>
                  </View>
                ) : null}
              </Pressable>
            )
          }}
        />
      )}
      <PickerSheet
        visible={pickAuthor}
        title="From"
        options={[{ key: '', label: 'Anyone', selected: authorId === null }, ...[...people].sort((a, b) => a.name.localeCompare(b.name)).map((person) => ({ key: person.id, label: person.name, selected: person.id === authorId }))]}
        onSelect={(key) => {
          setAuthorId(key || null)
          setPickAuthor(false)
        }}
        onClose={() => setPickAuthor(false)}
      />
    </ChatSidePane>
  )
}
