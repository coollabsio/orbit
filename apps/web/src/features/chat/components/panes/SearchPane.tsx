import { useState } from 'react'
import { Paperclip2, SearchNormal } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Toggle } from '@/components/ui/toggle'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useConversations, useSearchMessages } from '@/features/chat/api/queries'
import type { Conversation, SearchHit } from '@/features/chat/api/types'
import { decodeMentions } from '@/features/chat/lib/mentionTokens'
import { conversationTitle } from '@/features/chat/lib/sidebar'
import { useMembers } from '@/features/workspaces/api'
import type { User } from '@/features/workspaces/models'
import { leadToFirstMatch, splitByRanges } from './highlight'
import { MessageByline } from './MessageByline'
import { RightPane, RightPaneBody, RightPaneEmpty, RightPaneError, RightPaneLoading } from './RightPane'
import { useJumpToMessage } from './useJumpToMessage'

const ANYONE = 'anyone'

/** Search results for the query in the conversation header, with filters. A click on a result goes to the message. */
export function SearchPane({ conversationId, query }: { conversationId: string; query: string }) {
  const { workspaceId, currentUserId } = useChatContext()
  const members = useMembers(workspaceId).data ?? []
  const conversations = useConversations().data ?? []
  const jump = useJumpToMessage()
  const [thisConversation, setThisConversation] = useState(true)
  const [authorId, setAuthorId] = useState(ANYONE)
  const [hasFile, setHasFile] = useState(false)

  const text = query.trim()
  const search = useSearchMessages({
    query: text,
    conversationId: thisConversation ? conversationId : undefined,
    authorId: authorId === ANYONE ? undefined : authorId,
    hasFile: hasFile || undefined,
  })
  // the same rule as the hook: nothing is searched until there is a query or a filter
  const searching = text !== '' || authorId !== ANYONE || hasFile
  const hits = search.data?.pages.flatMap((page) => page.items) ?? []
  const authorOptions = [{ value: ANYONE, label: 'Anyone' }, ...[...members].sort((a, b) => a.name.localeCompare(b.name)).map((member) => ({ value: member.id, label: member.name }))]

  return (
    <RightPane title="Search">
      <div role="group" aria-label="Filters" className="flex shrink-0 flex-wrap items-center gap-1.5 border-b p-2">
        <Toggle variant="outline" size="sm" pressed={thisConversation} onPressedChange={setThisConversation}>
          This conversation
        </Toggle>
        <Select items={authorOptions} value={authorId} onValueChange={(value) => setAuthorId(value as string)}>
          <SelectTrigger size="sm" aria-label="From a member" className="w-auto max-w-44">
            <SelectValue>{(value: string) => (value === ANYONE ? 'From…' : `From ${members.find((member) => member.id === value)?.name ?? 'a member'}`)}</SelectValue>
          </SelectTrigger>
          <SelectContent align="start" alignItemWithTrigger={false} className="w-auto min-w-44">
            {authorOptions.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Toggle variant="outline" size="sm" pressed={hasFile} onPressedChange={setHasFile}>
          Has file
        </Toggle>
      </div>
      <RightPaneBody>
        {!searching ? (
          <RightPaneEmpty icon={SearchNormal} title="Search messages" description="Type in the search field of the conversation, or choose a filter." />
        ) : search.isPending ? (
          <RightPaneLoading />
        ) : search.isError ? (
          <RightPaneError title="The search results could not be loaded." onRetry={() => void search.refetch()} />
        ) : hits.length === 0 ? (
          <RightPaneEmpty
            icon={SearchNormal}
            title={text ? `No results for "${text}"` : 'No results'}
            description={thisConversation ? 'Nothing matches in this conversation.' : 'Try other words or remove a filter.'}
            action={
              thisConversation ? (
                <Button variant="outline" size="sm" onClick={() => setThisConversation(false)}>
                  Search all conversations
                </Button>
              ) : null
            }
          />
        ) : (
          <>
            <ul aria-label="Search results" className="flex flex-col gap-1">
              {hits.map((hit) => (
                <li key={hit.message.id}>
                  <SearchResult
                    hit={hit}
                    members={members}
                    conversations={conversations}
                    // a result names its conversation only when the search covers all of them
                    conversation={thisConversation ? undefined : conversations.find((conversation) => conversation.id === hit.conversationId)}
                    currentUserId={currentUserId ?? ''}
                    onOpen={() => jump(hit.message)}
                  />
                </li>
              ))}
            </ul>
            {search.hasNextPage ? (
              <Button variant="outline" size="sm" className="mx-auto mt-2" disabled={search.isFetchingNextPage} onClick={() => void search.fetchNextPage()}>
                {search.isFetchingNextPage ? 'Loading…' : 'Load more'}
              </Button>
            ) : null}
          </>
        )}
      </RightPaneBody>
    </RightPane>
  )
}

interface SearchResultProps {
  hit: SearchHit
  members: readonly User[]
  conversations: readonly Conversation[]
  conversation: Conversation | undefined
  currentUserId: string
  onOpen: () => void
}

/**
 * One result: the whole row is the button that goes to the message. The text is the message source with the matches
 * marked (`ranges` are offsets in the stored body), mentions shown as names.
 */
function SearchResult({ hit, members, conversations, conversation, currentUserId, onOpen }: SearchResultProps) {
  const { message } = hit
  const segments = leadToFirstMatch(splitByRanges(message.body, hit.ranges))
  // `#name` can point to a channel only; a DM has no name
  const channels = conversations.filter((item) => item.kind !== 'dm')
  const files = message.attachments.length
  return (
    <Button variant="ghost" className="h-auto w-full flex-col items-stretch gap-1 p-2 font-normal whitespace-normal active:not-aria-[haspopup]:translate-y-0" onClick={onOpen}>
      <MessageByline message={message} members={members}>
        {conversation ? (
          <span className="truncate text-xs text-muted-foreground">
            in {conversation.kind === 'dm' ? '' : '#'}
            {conversationTitle(conversation, members, currentUserId)}
          </span>
        ) : null}
      </MessageByline>
      {message.body ? (
        <span className="line-clamp-4 text-sm break-words whitespace-pre-wrap text-foreground">
          {segments.map((segment, index) =>
            segment.match ? (
              <mark key={index} className="rounded-xs bg-primary/20 text-foreground">
                {decodeMentions(segment.text, members, channels)}
              </mark>
            ) : (
              decodeMentions(segment.text, members, channels)
            ),
          )}
        </span>
      ) : null}
      {files > 0 ? (
        <span className="flex items-center gap-1 text-xs text-muted-foreground">
          <Paperclip2 aria-hidden className="size-3.5" />
          {files === 1 ? message.attachments[0].fileName : `${files} files`}
        </span>
      ) : null}
    </Button>
  )
}
