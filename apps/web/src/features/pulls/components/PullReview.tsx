import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { MessageText, Xmark } from 'reicon-react'
import type { GithubPullDetail, GithubReviewThread } from '@orbit/api/generated/types.gen'
import { asideReason, reviewSummary, threadsOutsideDiff, type PullRef } from '@orbit/api/github'
import { ApiProblem } from '@orbit/api/problem'
import { EmptyState } from '@/components/common/EmptyState'
import { Button } from '@/components/ui/button'
import { ConnectGithubPrompt } from '@/features/github/components/ConnectGithubPrompt'
import { needsGithubConnection, usePull, usePullConversation, usePullFiles, usePullThreads, useSetViewed } from '@/features/pulls/api'
import { PullActivity } from '@/features/pulls/components/PullActivity'
import { fileAnchor, PullDiffList } from '@/features/pulls/components/PullDiffList'
import { PullFileTree } from '@/features/pulls/components/PullFileTree'
import { PullKeyHints } from '@/features/pulls/components/PullKeyHints'
import { PullSummary } from '@/features/pulls/components/PullSummary'
import { PullTopBar } from '@/features/pulls/components/PullTopBar'
import { buildFileTree, treeOrder } from '@/features/pulls/lib/fileTree'
import { useDiffStyle } from '@/features/pulls/useDiffStyle'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useCommand } from '@/shortcuts/useCommand'

const ACTIVITY_KEY = 'orbit:pulls:activity'

/** The activity column stays as the reader left it; the first time it is open on a wide screen only. */
function useActivityOpen(): [boolean, (open: boolean) => void] {
  const [open, setOpen] = useState(() => {
    try {
      const saved = localStorage.getItem(ACTIVITY_KEY)
      if (saved !== null) return saved === '1'
    } catch { /* private mode */ }
    return typeof window !== 'undefined' && window.innerWidth >= 1280
  })
  const change = useCallback((next: boolean) => {
    setOpen(next)
    try { localStorage.setItem(ACTIVITY_KEY, next ? '1' : '0') } catch { /* the choice lasts this page */ }
  }, [])
  return [open, change]
}

/** One pull request inside the overlay, read with the member's own GitHub account. */
export function PullReview({ pullRef, commentId, onClose }: { pullRef: PullRef; commentId: number | null; onClose: () => void }) {
  const { workspace } = useWorkspace()
  const pull = usePull(workspace.id, pullRef)
  const closeButton = <Button variant="ghost" size="icon-sm" aria-label="Close" className="absolute top-2.5 right-2.5" onClick={onClose}><Xmark /></Button>
  if (needsGithubConnection(pull.error)) return <div className="relative flex flex-1 items-center justify-center">{closeButton}<ConnectGithubPrompt workspaceId={workspace.id} /></div>
  if (pull.error instanceof ApiProblem && pull.error.status === 404) return <div className="relative flex flex-1 items-center justify-center">{closeButton}<EmptyState icon={MessageText} title="Pull request not found" description="It does not exist, or your GitHub account cannot see it." /></div>
  if (pull.isError) return <div className="relative flex flex-1 items-center justify-center">{closeButton}<EmptyState icon={MessageText} title="Pull request unavailable" description="GitHub did not answer. Try again." action={<Button variant="outline" onClick={() => void pull.refetch()}>Retry</Button>} /></div>
  if (pull.data === undefined) return <ReviewSkeleton onClose={onClose} />
  return <ReviewBody workspaceId={workspace.id} pullRef={pullRef} pull={pull.data} commentId={commentId} onClose={onClose} />
}

/** The frame of the review while GitHub answers: the overlay opens at once, the content fills in. */
function ReviewSkeleton({ onClose }: { onClose: () => void }) {
  const bar = 'animate-pulse rounded bg-muted'
  return <div aria-busy="true" aria-label="Loading the pull request" className="flex min-h-0 flex-1 flex-col">
    <div className="flex h-12 shrink-0 items-center gap-3 border-b px-3">
      <span className={`${bar} h-5 w-12 rounded-full`} />
      <span className={`${bar} h-4 w-80 max-w-[50%]`} />
      <Button variant="ghost" size="icon-sm" aria-label="Close" className="ml-auto" onClick={onClose}><Xmark /></Button>
    </div>
    <div className="flex min-h-0 flex-1">
      <div className="flex w-68 shrink-0 flex-col gap-2.5 border-r p-3 max-[899px]:hidden">{[60, 85, 70, 50, 75].map((width) => <span key={width} className={`${bar} h-3`} style={{ width: `${width}%` }} />)}</div>
      <div className="flex flex-1 flex-col gap-3 bg-muted/30 p-4">{[1, 2].map((key) => <span key={key} className={`${bar} h-40 bg-muted/70`} />)}</div>
    </div>
  </div>
}

function ReviewBody({ workspaceId, pullRef, pull, commentId, onClose }: { workspaceId: string; pullRef: PullRef; pull: GithubPullDetail; commentId: number | null; onClose: () => void }) {
  const [diffStyle, setDiffStyle] = useDiffStyle()
  const [activityOpen, setActivityOpen] = useActivityOpen()
  const [reviewOpen, setReviewOpen] = useState(false)
  // The files shown belong to one head commit. A push refetches the header (live events), not the files: a bar
  // offers Refresh, so the diff never changes under the reader.
  const [shownHead, setShownHead] = useState(pull.head_sha)
  const files = usePullFiles(workspaceId, pullRef, shownHead)
  const threads = usePullThreads(workspaceId, pullRef)
  const conversation = usePullConversation(workspaceId, pullRef)
  const setViewed = useSetViewed(workspaceId, pullRef, shownHead)
  const [active, setActive] = useState<string | null>(null)
  // files opened by hand: a set-aside file, or a viewed (folded) file the reader went back to
  const [opened, setOpened] = useState<ReadonlySet<string>>(new Set())
  const open = useCallback((path: string, on: boolean) => setOpened((all) => {
    if (all.has(path) === on) return all
    const next = new Set(all)
    if (on) next.add(path)
    else next.delete(path)
    return next
  }), [])
  const [flash, setFlash] = useState<string | null>(null)
  const diffs = useRef<HTMLElement>(null)
  const filter = useRef<HTMLInputElement>(null)
  const newCommits = pull.head_sha !== shownHead

  // Files to review in the tree's order, then the files set aside (lockfiles, generated files, snapshots).
  const { review, aside } = useMemo(() => {
    const all = files.data ?? []
    return {
      review: treeOrder(buildFileTree(all.filter((file) => asideReason(file.path) === null))),
      aside: all.filter((file) => asideReason(file.path) !== null).sort((a, b) => a.path.localeCompare(b.path)),
    }
  }, [files.data])
  const ordered = useMemo(() => [...review, ...aside], [review, aside])
  const allThreads = useMemo(() => threads.data?.threads ?? [], [threads.data])
  const outside = useMemo(() => threadsOutsideDiff(allThreads, files.data ?? []), [allThreads, files.data])
  const summary = useMemo(() => reviewSummary({ files: files.data ?? [], threads: allThreads, timeline: conversation.data ?? [], viewer: pull.viewer.login, author: pull.author.login }), [files.data, allThreads, conversation.data, pull.viewer.login, pull.author.login])
  const linked = allThreads.find((thread) => thread.comments.some((comment) => comment.database_id === commentId))?.id ?? null

  const section = useCallback((path: string) => diffs.current?.querySelector<HTMLElement>(`#${CSS.escape(fileAnchor(path))}`) ?? null, [])
  // J/K move through the files as they are; a click (tree, thread) also opens a folded file
  const openFile = useCallback((path: string, unfold = false) => {
    setActive(path)
    if (unfold || asideReason(path) !== null) open(path, true)
    // once now, and once more after a file above folds (marked as viewed) or opens, so the file lands at the top
    requestAnimationFrame(() => section(path)?.scrollIntoView({ block: 'start' }))
    window.setTimeout(() => section(path)?.scrollIntoView({ block: 'start' }), 180)
  }, [section, open])

  // Jump to a thread in the diff (from the activity, or a notification), and let it show itself once.
  const jumpToThread = useCallback((thread: GithubReviewThread) => {
    if (threadsOutsideDiff([thread], files.data ?? []).length > 0) {
      setActivityOpen(true)
      requestAnimationFrame(() => document.getElementById('pull-outside-diff')?.scrollIntoView({ block: 'start' }))
      return
    }
    openFile(thread.path, true)
    setFlash(null)
    window.setTimeout(() => {
      diffs.current?.querySelector(`[data-thread-id="${CSS.escape(thread.id)}"]`)?.scrollIntoView({ block: 'center' })
      setFlash(thread.id)
    }, 250)
  }, [files.data, openFile, setActivityOpen])

  const jumpedTo = useRef<string | null>(null)
  useEffect(() => {
    if (!linked || jumpedTo.current === linked || !files.data) return
    const thread = allThreads.find((item) => item.id === linked)
    if (!thread) return
    jumpedTo.current = linked
    jumpToThread(thread)
  }, [linked, allThreads, files.data, jumpToThread])

  const step = (delta: number) => {
    if (ordered.length === 0) return
    const index = active ? ordered.findIndex((file) => file.path === active) : -1
    openFile(ordered[Math.min(ordered.length - 1, Math.max(0, index + delta))].path)
  }
  const stepThread = (delta: number) => {
    const box = diffs.current
    const nodes = [...(box?.querySelectorAll<HTMLElement>('[data-thread-id]') ?? [])]
    if (!box || nodes.length === 0) return
    const rect = box.getBoundingClientRect()
    const middle = rect.top + rect.height / 2
    const index = nodes.findIndex((node) => node.getBoundingClientRect().top > middle + 1)
    const next = delta > 0 ? (index === -1 ? nodes.length - 1 : index) : Math.max(0, (index === -1 ? nodes.length : index) - 2)
    nodes[next]?.scrollIntoView({ block: 'center' })
  }
  // V marks the file as viewed and moves on to the next file still to review, as a reviewer reads.
  const toggleViewed = () => {
    const file = ordered.find((item) => item.path === active) ?? ordered.find((item) => !item.viewed)
    if (!file) return
    setViewed.mutate({ file: file.path, viewed: !file.viewed })
    if (!file.viewed) open(file.path, false)
    if (file.viewed) return
    const after = ordered.slice(ordered.indexOf(file) + 1).find((item) => !item.viewed && asideReason(item.path) === null)
    if (after) openFile(after.path)
  }
  useCommand('pull.nextFile', () => step(1))
  useCommand('pull.prevFile', () => step(-1))
  useCommand('pull.nextThread', () => stepThread(1))
  useCommand('pull.prevThread', () => stepThread(-1))
  useCommand('pull.toggleViewed', toggleViewed, { enabled: ordered.length > 0 })
  useCommand('pull.activity', () => setActivityOpen(!activityOpen))
  useCommand('pull.filter', () => filter.current?.focus())
  useCommand('pull.review', () => setReviewOpen(true), { enabled: pull.viewer.can_review || (threads.data?.pending_review?.comment_count ?? 0) > 0 })

  return <div className="flex min-h-0 min-w-0 flex-1 flex-col">
    <PullTopBar
      workspaceId={workspaceId}
      pullRef={pullRef}
      pull={pull}
      progress={summary.toReview > 0 ? summary.viewed / summary.toReview : null}
      diffStyle={diffStyle}
      onDiffStyle={setDiffStyle}
      activityOpen={activityOpen}
      onActivity={() => setActivityOpen(!activityOpen)}
      reviewOpen={reviewOpen}
      onReviewOpen={setReviewOpen}
      onClose={onClose}
    />
    {newCommits ? <div role="status" className="flex items-center gap-3 border-b bg-primary/8 px-4 py-1.5 text-[13px]">
      <span>New commits were pushed.</span>
      <Button size="xs" variant="outline" onClick={() => { setShownHead(pull.head_sha); void threads.refetch() }}>Refresh</Button>
    </div> : null}
    <div className="flex min-h-0 flex-1">
      <aside aria-label="Review summary and files" className="flex w-68 shrink-0 flex-col border-r max-[899px]:hidden">
        <PullSummary pull={pull} summary={summary} onNextThread={() => stepThread(1)} />
        {files.data ? <PullFileTree
          review={review}
          aside={aside}
          threads={allThreads}
          active={active}
          filterRef={filter}
          onOpen={(path) => openFile(path, true)}
          onViewed={(path, viewed) => { setViewed.mutate({ file: path, viewed }); if (viewed) open(path, false) }}
        /> : <p className="p-3 text-xs text-muted-foreground">Loading files…</p>}
      </aside>
      <main ref={diffs} data-pull-diffs className="min-h-0 min-w-0 flex-1 bg-muted/35">
        {files.isError ? <EmptyState icon={MessageText} title="Files unavailable" description="GitHub did not send the changed files." action={<Button variant="outline" onClick={() => void files.refetch()}>Retry</Button>} />
          : files.data ? <PullDiffList
            workspaceId={workspaceId}
            pullRef={pullRef}
            pull={pull}
            headSha={shownHead}
            review={review}
            aside={aside}
            opened={opened}
            onOpen={(path) => open(path, true)}
            threads={allThreads}
            diffStyle={diffStyle}
            highlightedThread={flash}
          />
          : <p className="p-4 text-sm text-muted-foreground">Loading the changes…</p>}
      </main>
      {activityOpen ? <PullActivity
        workspaceId={workspaceId}
        pullRef={pullRef}
        pull={pull}
        threads={allThreads}
        outside={outside}
        timeline={conversation.data ?? []}
        loading={conversation.isPending}
        onJump={jumpToThread}
      /> : null}
    </div>
    <PullKeyHints />
  </div>
}
