import { useState } from 'react'
import { ChevronDown } from 'reicon-react'
import { toast } from 'sonner'
import type { GithubPullDetail } from '@orbit/api/generated/types.gen'
import { mergeDisabledReason, mergeMethodKey, pickMergeMethod, squashDefaults, type MergeMethod, type PullRef } from '@orbit/api/github'
import { ApiProblem } from '@orbit/api/problem'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { githubMessage, useDeleteBranch, useMerge } from '@/features/pulls/api'

const LABEL: Record<MergeMethod, string> = { merge: 'Create a merge commit', squash: 'Squash and merge', rebase: 'Rebase and merge' }
const BUTTON: Record<MergeMethod, string> = { merge: 'Merge', squash: 'Squash and merge', rebase: 'Rebase and merge' }

function stored(ref: PullRef): string | null {
  try { return localStorage.getItem(mergeMethodKey(ref)) } catch { return null }
}

/** Merge, as a split button: the main action is the method the member used last in this repository. */
export function MergeButton({ workspaceId, pullRef, pull }: { workspaceId: string; pullRef: PullRef; pull: GithubPullDetail }) {
  const [method, setMethod] = useState<MergeMethod | null>(() => pickMergeMethod(pull.merge, stored(pullRef)))
  const [confirming, setConfirming] = useState(false)
  const [commit, setCommit] = useState(() => squashDefaults(pull))
  const merge = useMerge(workspaceId, pullRef)
  const deleteBranch = useDeleteBranch(workspaceId, pullRef)
  if (pull.state !== 'open' && pull.state !== 'draft') return null
  const reason = mergeDisabledReason(pull)
  const headChanged = merge.error instanceof ApiProblem && merge.error.code === 'pull_head_changed'
  const choose = (next: MergeMethod) => {
    setMethod(next)
    try { localStorage.setItem(mergeMethodKey(pullRef), next) } catch { /* the choice lasts this page */ }
  }
  const run = (chosen: MergeMethod) => {
    const message = chosen === 'rebase' ? {} : { commit_title: commit.title, commit_message: commit.message }
    merge.mutate({ method: chosen, ...message, expected_head_sha: pull.head_sha }, {
      onSuccess: () => {
        setConfirming(false)
        toast.success('Merged', {
          action: { label: 'Delete branch', onClick: () => deleteBranch.mutate(undefined, {
            onSuccess: () => toast(`Deleted ${pull.head_ref}`),
            onError: (error) => toast.error(githubMessage(error, 'GitHub did not delete the branch.')),
          }) },
        })
      },
    })
  }
  const start = () => {
    if (!method) return
    // rebase merges at once; merge and squash let the member edit the commit first
    if (method === 'rebase') run(method)
    else { setCommit(method === 'squash' ? squashDefaults(pull) : { title: `Merge pull request #${pull.number} from ${pull.head_ref}`, message: pull.title }); setConfirming(true) }
  }
  const others = pull.merge.methods.filter((item): item is MergeMethod => item !== method && (item === 'merge' || item === 'squash' || item === 'rebase'))
  return <div className="flex flex-col items-end gap-1">
    <div className="flex">
      <Button size="sm" className={others.length > 0 ? 'rounded-r-none' : undefined} disabled={reason !== null || method === null || merge.isPending} onClick={start}>{method ? BUTTON[method] : 'Merge'}</Button>
      {others.length > 0 ? <DropdownMenu>
        <DropdownMenuTrigger render={<Button size="sm" className="rounded-l-none border-l border-primary-foreground/20 px-1.5" aria-label="More merge methods" disabled={reason !== null || merge.isPending} />}>
          <ChevronDown className="size-3.5" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {others.map((item) => <DropdownMenuItem key={item} onClick={() => choose(item)}>{LABEL[item]}</DropdownMenuItem>)}
        </DropdownMenuContent>
      </DropdownMenu> : null}
    </div>
    {reason ? <p className="max-w-64 text-right text-xs text-muted-foreground">{reason}</p> : null}
    {merge.isError && !confirming ? <p role="alert" className="max-w-64 text-right text-xs text-destructive">{headChanged ? 'New commits were pushed. Refresh before you merge.' : githubMessage(merge.error, 'GitHub did not merge. Try again.')}</p> : null}
    <Dialog open={confirming} onOpenChange={setConfirming}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{method ? LABEL[method] : 'Merge'}</DialogTitle>
          <DialogDescription>Merge {pull.head_ref} into {pull.base_ref}.</DialogDescription>
        </DialogHeader>
        <label className="flex flex-col gap-1.5 text-xs font-medium text-muted-foreground">Commit title
          <Input value={commit.title} onChange={(event) => setCommit({ ...commit, title: event.target.value })} />
        </label>
        <label className="flex flex-col gap-1.5 text-xs font-medium text-muted-foreground">Commit message
          <Textarea value={commit.message} onChange={(event) => setCommit({ ...commit, message: event.target.value })} className="min-h-28" />
        </label>
        {merge.isError ? <p role="alert" className="text-xs text-destructive">{headChanged ? 'New commits were pushed. Refresh before you merge.' : githubMessage(merge.error, 'GitHub did not merge. Try again.')}</p> : null}
        <DialogFooter>
          <Button variant="outline" onClick={() => setConfirming(false)}>Cancel</Button>
          <Button aria-label={`Confirm ${method ? LABEL[method].toLowerCase() : 'merge'}`} disabled={!commit.title.trim() || merge.isPending} onClick={() => method && run(method)}>{method ? BUTTON[method] : 'Merge'}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
}
