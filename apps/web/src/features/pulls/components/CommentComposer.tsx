import { useId } from 'react'
import type { CommentTarget, PullRef } from '@orbit/api/github'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { githubMessage, useAddComment, usePullThreads } from '@/features/pulls/api'
import { draftKey, useDraft } from '@/features/pulls/lib/drafts'
import { shortcutText } from '@/shortcuts/shortcutText'

const linesLabel = (target: CommentTarget) => target.start_line != null && target.start_line !== target.line ? `lines ${target.start_line} to ${target.line}` : `line ${target.line}`

/** The comment box under a line or a range. The draft is kept until it is posted or discarded. */
export function CommentComposer({ workspaceId, pullRef, headSha, target, onDone }: { workspaceId: string; pullRef: PullRef; headSha: string; target: CommentTarget; onDone: () => void }) {
  const [text, setText, clear] = useDraft(draftKey(workspaceId, pullRef, target))
  const add = useAddComment(workspaceId, pullRef)
  // GitHub allows one pending review: while it is open, a comment goes into it (as on GitHub)
  const reviewing = (usePullThreads(workspaceId, pullRef).data?.pending_review ?? null) !== null
  const id = useId()
  const post = (mode: 'pending' | 'single') => {
    if (!text.trim() || add.isPending) return
    add.mutate({ target, body: text, mode, headSha }, { onSuccess: () => { clear(); onDone() } })
  }
  return <div className="my-1 flex flex-col gap-2 rounded-lg border bg-card p-2.5 font-sans shadow-xs" data-composer>
    <label htmlFor={id} className="sr-only">Comment on {linesLabel(target)}</label>
    <Textarea
      id={id}
      autoFocus
      value={text}
      onChange={(event) => setText(event.target.value)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) { event.preventDefault(); post('pending') }
        if (event.key === 'Escape') { event.preventDefault(); onDone() }
      }}
      placeholder={`Comment on ${linesLabel(target)}`}
      className="min-h-20 text-[13px]"
    />
    {add.isError ? <p role="alert" className="text-xs text-destructive">{githubMessage(add.error, 'GitHub did not take the comment. Try again.')}</p> : null}
    <div className="flex flex-wrap items-center gap-2">
      <Button size="sm" disabled={!text.trim() || add.isPending} onClick={() => post('pending')}>Add to review</Button>
      {reviewing ? null : <Button size="sm" variant="outline" disabled={!text.trim() || add.isPending} onClick={() => post('single')}>Comment now</Button>}
      <span className="text-xs text-muted-foreground">{shortcutText('Mod+Enter')} adds to review</span>
      <div className="ml-auto flex gap-1">
        {text ? <Button size="sm" variant="ghost" onClick={() => { clear(); onDone() }}>Discard</Button> : null}
        <Button size="sm" variant="ghost" onClick={onDone}>{text ? 'Close' : 'Cancel'}</Button>
      </div>
    </div>
  </div>
}
