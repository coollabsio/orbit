// What a tinted reference in the message box is, in the hover card: the member (and whether they will be notified),
// the channel, "@channel"/"@here", or the task an ID names.
import { Hashtag, Lock } from 'reicon-react'
import { PersonPreview, PreviewNote, TaskPreview } from '@/components/common/ReferencePreview'
import { useConversations } from '../../api/queries'
import type { Conversation } from '../../api/types'

const GLOBAL_NOTE: Record<string, string> = {
  channel: 'Notifies everyone in this conversation.',
  here: 'Notifies everyone in this conversation who is online.',
}

export function ComposerReferencePreview({ reference, conversation }: { reference: string; conversation: Pick<Conversation, 'kind' | 'memberIds'> }) {
  const split = reference.indexOf(':')
  const kind = reference.slice(0, split)
  const id = reference.slice(split + 1)
  if (kind === 'task') return <TaskPreview taskRef={id} missingNote="It stays plain text in the message." />
  if (kind === 'channel') return <ChannelPreview channelId={id} />
  if (kind === 'global') {
    return (
      <>
        <div className="font-medium">@{id}</div>
        <p className="mt-1 text-xs text-muted-foreground">{GLOBAL_NOTE[id] ?? ''}</p>
      </>
    )
  }
  // the same rule as the "@" list: everybody in a public channel, the members elsewhere
  const notified = conversation.kind === 'public' || conversation.memberIds.includes(id)
  return <PersonPreview userId={id} note={notified ? null : <PreviewNote tone="warning">Not in this conversation, so they get no notification.</PreviewNote>} />
}

function ChannelPreview({ channelId }: { channelId: string }) {
  const channel = useConversations().data?.find((candidate) => candidate.id === channelId)
  if (!channel) return <div className="text-muted-foreground">Unknown channel</div>
  const Icon = channel.kind === 'private' ? Lock : Hashtag
  return (
    <>
      <div className="flex items-center gap-1.5 font-medium">
        <Icon className="size-4 shrink-0 text-muted-foreground" />
        <span className="truncate">{channel.name}</span>
      </div>
      {channel.topic ? <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{channel.topic}</p> : null}
      <p className="mt-2 text-xs text-muted-foreground">
        {channel.kind === 'private' ? 'Private channel' : 'Channel'} · {channel.memberIds.length} {channel.memberIds.length === 1 ? 'member' : 'members'}
        {channel.archived ? ' · Archived' : ''}
      </p>
    </>
  )
}
