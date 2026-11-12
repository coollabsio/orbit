import { useId, useState } from 'react'
import type { GithubPullViewer } from '@orbit/api/generated/types.gen'
import type { PullRef } from '@orbit/api/github'
import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Textarea } from '@/components/ui/textarea'
import { githubMessage, useDiscardReview, usePullThreads, useSubmitReview } from '@/features/pulls/api'
import { shortcutText } from '@/shortcuts/shortcutText'

type Event = 'COMMENT' | 'APPROVE' | 'REQUEST_CHANGES'
const DONE: Record<Event, string> = { COMMENT: 'Review sent', APPROVE: 'Approved', REQUEST_CHANGES: 'Changes requested' }

/** The Review button: a summary, and one click to comment, request changes or approve. The member's pending comments
 *  go with it. `open` is the parent's when R opens it. */
export function ReviewPanel({ workspaceId, pullRef, viewer, open: openProp, onOpenChange }: {
  workspaceId: string
  pullRef: PullRef
  viewer: GithubPullViewer
  open?: boolean
  onOpenChange?: (open: boolean) => void
}) {
  const [ownOpen, setOwnOpen] = useState(false)
  const open = openProp ?? ownOpen
  const setOpen = onOpenChange ?? setOwnOpen
  const [body, setBody] = useState('')
  const [sending, setSending] = useState<Event | null>(null)
  const threads = usePullThreads(workspaceId, pullRef)
  const submit = useSubmitReview(workspaceId, pullRef)
  const discard = useDiscardReview(workspaceId, pullRef)
  const pending = threads.data?.pending_review?.comment_count ?? 0
  const id = useId()
  if (!viewer.can_review && pending === 0) return null
  // GitHub refuses an empty comment or change request; an approval may be empty
  const empty = !body.trim() && pending === 0
  const send = (event: Event) => {
    if ((event !== 'APPROVE' && empty) || submit.isPending) return
    setSending(event)
    submit.mutate({ event, body }, {
      onSuccess: () => {
        setOpen(false)
        setBody('')
        void threads.refetch()
        toast.success(DONE[event])
      },
      onSettled: () => setSending(null),
    })
  }
  const askDiscard = async () => {
    const confirmed = await confirmAction({ title: 'Discard your pending review?', description: `Your ${pending} pending ${pending === 1 ? 'comment is' : 'comments are'} deleted on GitHub too.`, confirmLabel: 'Discard', danger: true })
    if (confirmed) discard.mutate(undefined, { onSuccess: () => setOpen(false) })
  }
  const decide = viewer.can_review && !viewer.is_author
  return <Popover open={open} onOpenChange={setOpen}>
    <PopoverTrigger render={<Button size="sm" variant={pending > 0 ? 'default' : 'outline'} aria-label={pending > 0 ? `Review, ${pending} pending comments` : 'Review'} />}>
      Review{pending > 0 ? <span className="ml-0.5 rounded-full bg-background/25 px-1.5 text-[11px] tabular-nums">{pending}</span> : null}
    </PopoverTrigger>
    <PopoverContent align="end" className="flex w-[26rem] max-w-[calc(100vw-2rem)] flex-col gap-2.5 p-3">
      <label htmlFor={id} className="sr-only">Review summary</label>
      <Textarea id={id} autoFocus value={body} onChange={(changed) => setBody(changed.target.value)} placeholder={decide ? 'Summary (optional for an approval)' : 'Summary'} className="min-h-24 text-[13px]"
        onKeyDown={(key) => { if (key.key === 'Enter' && (key.metaKey || key.ctrlKey)) { key.preventDefault(); send('COMMENT') } }} />
      {pending > 0 ? <p className="text-xs text-muted-foreground">Your {pending} pending {pending === 1 ? 'comment goes' : 'comments go'} with the review.</p> : null}
      {empty && !decide ? <p className="text-xs text-muted-foreground">Write a summary or add comments first.</p> : null}
      {submit.isError ? <p role="alert" className="text-xs text-destructive">{githubMessage(submit.error, 'GitHub did not take the review. Try again.')}</p> : null}
      <div className="flex flex-wrap items-center gap-1.5">
        {pending > 0 ? <Button size="sm" variant="ghost" className="text-destructive" disabled={discard.isPending} onClick={() => void askDiscard()}>Discard</Button> : null}
        <span className="mr-auto" />
        <Button size="sm" variant="ghost" title={`Comment (${shortcutText('Mod+Enter')})`} disabled={empty || submit.isPending} onClick={() => send('COMMENT')}>{sending === 'COMMENT' ? 'Sending…' : 'Comment'}</Button>
        {decide ? <>
          <Button size="sm" variant="outline" disabled={empty || submit.isPending} onClick={() => send('REQUEST_CHANGES')}>{sending === 'REQUEST_CHANGES' ? 'Sending…' : 'Request changes'}</Button>
          <Button size="sm" disabled={submit.isPending} onClick={() => send('APPROVE')} className="bg-emerald-600 text-white hover:bg-emerald-600/90">{sending === 'APPROVE' ? 'Approving…' : 'Approve'}</Button>
        </> : null}
      </div>
    </PopoverContent>
  </Popover>
}
