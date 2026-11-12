import { useId, useState } from 'react'
import type { GithubReviewComment, GithubReviewThread } from '@orbit/api/generated/types.gen'
import type { PullRef } from '@orbit/api/github'
import { cn } from 'cn'
import { confirmAction } from '@/components/common/confirmAction'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { githubMessage, useDeleteComment, useEditComment, useReply, useResolveThread } from '@/features/pulls/api'
import { GithubAvatar } from '@/features/pulls/components/PullBits'
import { replyDraftKey, useDraft } from '@/features/pulls/lib/drafts'
import { relativeTime } from '@/lib/format'
import { renderMarkdownBlocks } from '@/lib/markdown'

type Actions = { workspaceId: string; pullRef: PullRef }

/** One review thread: its comments, and whether it is resolved, outdated or pending. With `actions`: reply, resolve,
 *  edit and delete own comments. */
export function ThreadView({ thread, highlighted = false, actions }: { thread: GithubReviewThread; highlighted?: boolean; actions?: Actions }) {
  return <div data-thread-id={thread.id} data-thread-flash={highlighted || undefined} className={cn('my-1 flex flex-col gap-2 rounded-lg border bg-card px-3 py-2.5 font-sans text-[13px] text-foreground shadow-xs', thread.is_resolved && 'opacity-80')}>
    {thread.is_resolved || thread.is_outdated ? <div className="flex gap-1.5">
      {thread.is_resolved ? <span className="rounded bg-muted px-1.5 text-[11px] text-muted-foreground">Resolved</span> : null}
      {thread.is_outdated ? <span className="rounded bg-muted px-1.5 text-[11px] text-muted-foreground">Outdated</span> : null}
    </div> : null}
    {thread.comments.map((comment) => <Comment key={comment.id} comment={comment} actions={actions} />)}
    {actions ? <ThreadActions thread={thread} actions={actions} /> : null}
  </div>
}

function Comment({ comment, actions }: { comment: GithubReviewComment; actions?: Actions }) {
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState(comment.body)
  const edit = useEditComment(actions?.workspaceId ?? '', actions?.pullRef ?? { owner: '', repo: '', number: 0 })
  const remove = useDeleteComment(actions?.workspaceId ?? '', actions?.pullRef ?? { owner: '', repo: '', number: 0 })
  const canEdit = actions !== undefined && comment.can_edit
  const askDelete = async () => {
    const confirmed = await confirmAction({ title: 'Delete this comment?', description: 'It is deleted on GitHub too.', confirmLabel: 'Delete', danger: true })
    if (confirmed) remove.mutate(comment.id)
  }
  return <div data-comment-id={comment.database_id} className="group/comment flex gap-2">
    <GithubAvatar actor={comment.author} size={22} />
    <div className="min-w-0 flex-1">
      <div className="flex items-baseline gap-1.5">
        <span className="font-medium">{comment.author.login}</span>
        <span className="text-xs text-muted-foreground">{relativeTime(new Date(comment.created_at).toISOString())}</span>
        {comment.pending ? <span className="rounded bg-amber-500/15 px-1.5 text-[11px] text-amber-700 dark:text-amber-400">Pending</span> : null}
        {canEdit && !editing ? <span className="ml-auto flex gap-1 opacity-0 transition-opacity group-hover/comment:opacity-100 focus-within:opacity-100 pointer-coarse:opacity-100">
          <Button size="xs" variant="ghost" onClick={() => { setText(comment.body); setEditing(true) }}>Edit</Button>
          <Button size="xs" variant="ghost" aria-label="Delete comment" disabled={remove.isPending} onClick={() => void askDelete()}>Delete</Button>
        </span> : null}
      </div>
      {editing ? <div className="mt-1 flex flex-col gap-1.5">
        <Textarea aria-label="Edit comment" value={text} onChange={(event) => setText(event.target.value)} className="min-h-16 text-[13px]" autoFocus />
        {edit.isError ? <p role="alert" className="text-xs text-destructive">{githubMessage(edit.error, 'GitHub did not take the change.')}</p> : null}
        <div className="flex gap-1.5">
          <Button size="sm" disabled={!text.trim() || edit.isPending} onClick={() => edit.mutate({ commentId: comment.id, body: text }, { onSuccess: () => setEditing(false) })}>Save</Button>
          <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>Cancel</Button>
        </div>
      </div> : <div className="mt-0.5 break-words [&_p]:my-1">{renderMarkdownBlocks(comment.body, comment.id)}</div>}
      {remove.isError ? <p role="alert" className="text-xs text-destructive">{githubMessage(remove.error, 'GitHub did not delete the comment.')}</p> : null}
    </div>
  </div>
}

function ThreadActions({ thread, actions }: { thread: GithubReviewThread; actions: Actions }) {
  const [text, setText, clear] = useDraft(replyDraftKey(actions.workspaceId, actions.pullRef, thread.id))
  // a saved draft opens the box again; Close hides it and keeps the draft
  const [replying, setReplying] = useState(() => text !== '')
  const reply = useReply(actions.workspaceId, actions.pullRef)
  const resolve = useResolveThread(actions.workspaceId, actions.pullRef)
  const id = useId()
  const send = () => {
    if (!text.trim() || reply.isPending) return
    reply.mutate({ threadId: thread.id, body: text }, { onSuccess: () => { clear(); setReplying(false) } })
  }
  return <div className="flex flex-col gap-1.5 border-t pt-2">
    {replying ? <>
      <label htmlFor={id} className="sr-only">Reply to the thread</label>
      <Textarea id={id} autoFocus value={text} onChange={(event) => setText(event.target.value)} placeholder="Reply" className="min-h-14 text-[13px]"
        onKeyDown={(event) => { if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) { event.preventDefault(); send() } }} />
      {reply.isError ? <p role="alert" className="text-xs text-destructive">{githubMessage(reply.error, 'GitHub did not take the reply. Try again.')}</p> : null}
    </> : null}
    <div className="flex flex-wrap gap-1.5">
      {thread.can_reply ? (replying
        ? <>
          <Button size="sm" disabled={!text.trim() || reply.isPending} onClick={send}>Send reply</Button>
          <Button size="sm" variant="ghost" onClick={() => setReplying(false)}>Close</Button>
        </>
        : <Button size="sm" variant="outline" onClick={() => setReplying(true)}>{text ? 'Continue reply' : 'Reply'}</Button>) : null}
      {thread.can_resolve ? <Button size="sm" variant="ghost" className="ml-auto" disabled={resolve.isPending} onClick={() => resolve.mutate({ threadId: thread.id, resolved: !thread.is_resolved })}>{thread.is_resolved ? 'Unresolve' : 'Resolve'}</Button> : null}
    </div>
    {resolve.isError ? <p role="alert" className="text-xs text-destructive">{githubMessage(resolve.error, 'GitHub did not change the thread.')}</p> : null}
  </div>
}
