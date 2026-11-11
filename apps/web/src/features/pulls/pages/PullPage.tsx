import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useParams, useSearchParams } from 'react-router'
import { MessageText } from 'reicon-react'
import type { GithubPullDetail } from '@orbit/api/generated/types.gen'
import { threadsOutsideDiff, type PullRef } from '@orbit/api/github'
import { ApiProblem } from '@orbit/api/problem'
import { EmptyState } from '@/components/common/EmptyState'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { ConnectGithubPrompt } from '@/features/github/components/ConnectGithubPrompt'
import { needsGithubConnection, usePull, usePullConversation, usePullFiles, usePullThreads, useSetViewed } from '@/features/pulls/api'
import { fileAnchor, PullDiffList } from '@/features/pulls/components/PullDiffList'
import { PullFileTree } from '@/features/pulls/components/PullFileTree'
import { MergeButton } from '@/features/pulls/components/MergeButton'
import { PullHeader } from '@/features/pulls/components/PullHeader'
import { PullOverview } from '@/features/pulls/components/PullOverview'
import { ReviewPanel } from '@/features/pulls/components/ReviewPanel'
import { buildFileTree, treeOrder } from '@/features/pulls/lib/fileTree'
import { useDiffStyle } from '@/features/pulls/useDiffStyle'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useCommand } from '@/shortcuts/useCommand'

/** `/tasks/pulls/:owner/:repo/:number`: one pull request, read with the member's own GitHub account. */
export function PullPage() {
  const { workspace } = useWorkspace()
  const { owner = '', repo = '', number = '' } = useParams()
  const ref = useMemo<PullRef>(() => ({ owner, repo, number: Number(number) }), [owner, repo, number])
  const pull = usePull(workspace.id, ref)
  if (needsGithubConnection(pull.error)) return <div className="flex flex-1 items-center justify-center"><ConnectGithubPrompt workspaceId={workspace.id} /></div>
  if (pull.error instanceof ApiProblem && pull.error.status === 404) return <EmptyState icon={MessageText} title="Pull request not found" description="It does not exist, or your GitHub account cannot see it." />
  if (pull.isError) return <EmptyState icon={MessageText} title="Pull request unavailable" description="GitHub did not answer. Try again." action={<Button variant="outline" onClick={() => void pull.refetch()}>Retry</Button>} />
  if (pull.data === undefined) return <p className="p-6 text-sm text-muted-foreground">Loading the pull request…</p>
  return <PullView workspaceId={workspace.id} pullRef={ref} pull={pull.data} />
}

function PullView({ workspaceId, pullRef, pull }: { workspaceId: string; pullRef: PullRef; pull: GithubPullDetail }) {
  const [tab, setTab] = useState<'files' | 'overview'>('files')
  const [diffStyle, setDiffStyle] = useDiffStyle()
  const files = usePullFiles(workspaceId, pullRef)
  const threads = usePullThreads(workspaceId, pullRef)
  const conversation = usePullConversation(workspaceId, pullRef, tab === 'overview')
  const setViewed = useSetViewed(workspaceId, pullRef)
  const [params] = useSearchParams()
  const [active, setActive] = useState<string | null>(null)
  const list = useRef<HTMLDivElement>(null)
  // The files shown belong to one head commit. A push refetches the header (live events), not the files: a bar
  // offers Refresh, so the diff never changes under the reader.
  const head = useRef(pull.head_sha)
  head.current = pull.head_sha
  const [filesHead, setFilesHead] = useState(pull.head_sha)
  useEffect(() => { if (files.dataUpdatedAt) setFilesHead(head.current) }, [files.dataUpdatedAt])
  const newCommits = files.data !== undefined && pull.head_sha !== filesHead
  // the diff list shows the files in the tree's order
  const ordered = useMemo(() => treeOrder(buildFileTree(files.data ?? [])), [files.data])
  const allThreads = useMemo(() => threads.data?.threads ?? [], [threads.data])
  const outside = useMemo(() => threadsOutsideDiff(allThreads, files.data ?? []), [allThreads, files.data])
  const commentId = Number(params.get('comment'))
  const highlighted = allThreads.find((thread) => thread.comments.some((comment) => comment.database_id === commentId))?.id ?? null

  const openFile = useCallback((path: string) => {
    setTab('files')
    setActive(path)
    requestAnimationFrame(() => document.getElementById(fileAnchor(path))?.scrollIntoView({ block: 'start' }))
  }, [])

  // A notification opens a thread: go to its file once, when the threads are in.
  const opened = useRef<string | null>(null)
  useEffect(() => {
    if (!highlighted || opened.current === highlighted) return
    const thread = allThreads.find((item) => item.id === highlighted)
    if (!thread || !files.data) return
    opened.current = highlighted
    if (threadsOutsideDiff([thread], files.data).length > 0) { setTab('overview'); return }
    openFile(thread.path)
    window.setTimeout(() => document.querySelector(`[data-thread-id="${CSS.escape(highlighted)}"]`)?.scrollIntoView({ block: 'center' }), 300)
  }, [highlighted, allThreads, files.data, openFile])

  const step = (delta: number) => {
    if (ordered.length === 0) return
    const index = active ? ordered.findIndex((file) => file.path === active) : -1
    openFile(ordered[Math.min(ordered.length - 1, Math.max(0, index + delta))].path)
  }
  const stepThread = (delta: number) => {
    const nodes = [...(list.current?.closest('[data-pull-diffs]')?.querySelectorAll<HTMLElement>('[data-thread-id]') ?? [])]
    if (nodes.length === 0) return
    const middle = window.innerHeight / 2
    const index = nodes.findIndex((node) => node.getBoundingClientRect().top > middle + 1)
    const next = delta > 0 ? (index === -1 ? nodes.length - 1 : index) : Math.max(0, (index === -1 ? nodes.length : index) - 2)
    nodes[next]?.scrollIntoView({ block: 'center' })
  }
  const toggleViewed = () => {
    const file = ordered.find((item) => item.path === active)
    if (file) setViewed.mutate({ file: file.path, viewed: !file.viewed })
  }
  const onFiles = tab === 'files'
  useCommand('pull.nextFile', () => step(1), { enabled: onFiles })
  useCommand('pull.prevFile', () => step(-1), { enabled: onFiles })
  useCommand('pull.nextThread', () => stepThread(1), { enabled: onFiles })
  useCommand('pull.prevThread', () => stepThread(-1), { enabled: onFiles })
  useCommand('pull.toggleViewed', toggleViewed, { enabled: onFiles && active !== null })

  return <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-background">
    <PullHeader pull={pull} actions={<>
      <ReviewPanel workspaceId={workspaceId} pullRef={pullRef} viewer={pull.viewer} />
      <MergeButton workspaceId={workspaceId} pullRef={pullRef} pull={pull} />
    </>} />
    <div className="flex items-center gap-3 border-b px-4 py-1.5">
      <Tabs value={tab} onValueChange={(value) => setTab(value as 'files' | 'overview')}>
        <TabsList>
          <TabsTrigger value="files">Files {files.data ? <span className="ml-1 text-muted-foreground tabular-nums">{files.data.length}</span> : null}</TabsTrigger>
          <TabsTrigger value="overview">Overview</TabsTrigger>
        </TabsList>
      </Tabs>
      {onFiles ? <Tabs value={diffStyle} onValueChange={(value) => setDiffStyle(value as 'split' | 'unified')} className="ml-auto">
        <TabsList aria-label="Diff layout">
          <TabsTrigger value="split">Split</TabsTrigger>
          <TabsTrigger value="unified">Unified</TabsTrigger>
        </TabsList>
      </Tabs> : null}
    </div>
    {newCommits ? <div role="status" className="flex items-center gap-3 border-b bg-primary/8 px-4 py-1.5 text-[13px]">
      <span>New commits were pushed.</span>
      <Button size="xs" variant="outline" onClick={() => { void files.refetch(); void threads.refetch() }}>Refresh</Button>
    </div> : null}
    {onFiles ? <div className="flex min-h-0 flex-1">
      <aside className="flex w-72 shrink-0 flex-col border-r max-[899px]:hidden">
        {files.data ? <PullFileTree files={files.data} active={active} onOpen={openFile} onViewed={(path, viewed) => setViewed.mutate({ file: path, viewed })} /> : <p className="p-3 text-xs text-muted-foreground">Loading files…</p>}
      </aside>
      <main data-pull-diffs className="min-h-0 min-w-0 flex-1">
        {files.isError ? <EmptyState icon={MessageText} title="Files unavailable" description="GitHub did not send the changed files." action={<Button variant="outline" onClick={() => void files.refetch()}>Retry</Button>} />
          : files.data ? <PullDiffList ref={list} workspaceId={workspaceId} pullRef={pullRef} pull={pull} files={ordered} threads={allThreads} diffStyle={diffStyle} highlightedThread={highlighted} />
          : <p className="p-4 text-sm text-muted-foreground">Loading the changes…</p>}
      </main>
    </div> : <div className="min-h-0 flex-1 overflow-y-auto"><PullOverview pull={pull} timeline={conversation.data ?? []} outside={outside} /></div>}
  </div>
}
