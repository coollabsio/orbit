import { useId, useState } from 'react'
import type { GithubPullViewer } from '@orbit/api/generated/types.gen'
import type { PullRef } from '@orbit/api/github'
import { toast } from 'sonner'
import { confirmAction } from '@/components/common/confirmAction'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Textarea } from '@/components/ui/textarea'
import { githubMessage, useDiscardReview, usePullThreads, useSubmitReview } from '@/features/pulls/api'

type Event = 'COMMENT' | 'APPROVE' | 'REQUEST_CHANGES'
const EVENTS: { value: Event; label: string; hint: string }[] = [
  { value: 'COMMENT', label: 'Comment', hint: 'Feedback without approval.' },
  { value: 'APPROVE', label: 'Approve', hint: 'Give your approval to merge.' },
  { value: 'REQUEST_CHANGES', label: 'Request changes', hint: 'Feedback that must be addressed first.' },
]

/** The Review button: a summary, Comment / Approve / Request changes, and the member's pending comments with it. */
export function ReviewPanel({ workspaceId, pullRef, viewer }: { workspaceId: string; pullRef: PullRef; viewer: GithubPullViewer }) {
  const [open, setOpen] = useState(false)
  const [event, setEvent] = useState<Event>('COMMENT')
  const [body, setBody] = useState('')
  const threads = usePullThreads(workspaceId, pullRef)
  const submit = useSubmitReview(workspaceId, pullRef)
  const discard = useDiscardReview(workspaceId, pullRef)
  const pending = threads.data?.pending_review?.comment_count ?? 0
  const id = useId()
  if (!viewer.can_review && pending === 0) return null
  const events = EVENTS.filter((item) => !viewer.is_author || item.value === 'COMMENT')
  const empty = event !== 'APPROVE' && !body.trim() && pending === 0
  const send = () => {
    if (empty || submit.isPending) return
    submit.mutate({ event, body }, {
      onSuccess: () => {
        setOpen(false)
        setBody('')
        void threads.refetch()
        toast.success(event === 'APPROVE' ? 'Approved' : event === 'REQUEST_CHANGES' ? 'Changes requested' : 'Review sent')
      },
    })
  }
  const askDiscard = async () => {
    const confirmed = await confirmAction({ title: 'Discard your pending review?', description: `Your ${pending} pending ${pending === 1 ? 'comment is' : 'comments are'} deleted on GitHub too.`, confirmLabel: 'Discard', danger: true })
    if (confirmed) discard.mutate(undefined, { onSuccess: () => setOpen(false) })
  }
  return <Popover open={open} onOpenChange={setOpen}>
    <PopoverTrigger render={<Button size="sm" variant={pending > 0 ? 'default' : 'outline'} aria-label={pending > 0 ? `Review, ${pending} pending comments` : 'Review'} />}>
      Review{pending > 0 ? <span className="ml-0.5 rounded-full bg-background/25 px-1.5 text-[11px] tabular-nums">{pending}</span> : null}
    </PopoverTrigger>
    <PopoverContent align="end" className="flex w-96 flex-col gap-3 p-3">
      <label htmlFor={id} className="text-xs font-medium text-muted-foreground">Review summary</label>
      <Textarea id={id} autoFocus value={body} onChange={(changed) => setBody(changed.target.value)} placeholder="Leave a comment" className="min-h-24 text-[13px]"
        onKeyDown={(key) => { if (key.key === 'Enter' && (key.metaKey || key.ctrlKey)) { key.preventDefault(); send() } }} />
      <fieldset className="flex flex-col gap-1.5">
        <legend className="sr-only">Review result</legend>
        {events.map((item) => <label key={item.value} className="flex cursor-pointer items-start gap-2 rounded-md px-1.5 py-1 hover:bg-muted">
          <input type="radio" name={`${id}-event`} value={item.value} checked={event === item.value} onChange={() => setEvent(item.value)} aria-label={item.label} aria-describedby={`${id}-${item.value}`} className="mt-0.5 accent-primary" />
          <span className="flex flex-col"><span className="text-[13px] font-medium">{item.label}</span><span id={`${id}-${item.value}`} className="text-xs text-muted-foreground">{item.hint}</span></span>
        </label>)}
      </fieldset>
      {empty ? <p className="text-xs text-muted-foreground">Write a summary or add comments first.</p> : null}
      {submit.isError ? <p role="alert" className="text-xs text-destructive">{githubMessage(submit.error, 'GitHub did not take the review. Try again.')}</p> : null}
      <div className="flex items-center gap-2">
        {pending > 0 ? <Button size="sm" variant="ghost" className="text-destructive" disabled={discard.isPending} onClick={() => void askDiscard()}>Discard pending review</Button> : null}
        <Button size="sm" className="ml-auto" disabled={empty || submit.isPending} onClick={send}>Submit review</Button>
      </div>
    </PopoverContent>
  </Popover>
}
