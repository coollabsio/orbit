import { useState } from 'react'
import { useParams } from 'react-router'
import { MessageText } from 'reicon-react'
import type { GithubPullSummary } from '@orbit/api/generated/types.gen'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Button } from '@/components/ui/button'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { ConnectGithubPrompt } from '@/features/github/components/ConnectGithubPrompt'
import { needsGithubConnection, type PullFilter, usePullList } from '@/features/pulls/api'
import { GithubAvatar, PullStateBadge } from '@/features/pulls/components/PullBits'
import { useProjects } from '@/features/tasks/api/projects'
import { TaskSearchBox } from '@/features/tasks/components/TaskSearchBox'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { relativeTime } from '@/lib/format'
import { useDebouncedValue } from '@/lib/useDebouncedValue'
import { PullLink } from '@/features/pulls/components/PullLink'

const FILTERS: { value: PullFilter; label: string; empty: string }[] = [
  { value: 'open', label: 'Open', empty: 'No open pull requests' },
  { value: 'review', label: 'Needs my review', empty: 'Nothing waits for your review' },
  { value: 'mine', label: 'Mine', empty: 'You have no open pull requests' },
  { value: 'merged', label: 'Merged', empty: 'No merged pull requests' },
  { value: 'closed', label: 'Closed', empty: 'No closed pull requests' },
]

/** A project's pull requests, read live from GitHub with the member's own account. */
export function PullListPage() {
  const { projectId = '' } = useParams()
  // another project starts fresh: its own first page, the Open filter, no search
  return <ProjectPulls key={projectId} projectId={projectId} />
}

function ProjectPulls({ projectId }: { projectId: string }) {
  const { workspace } = useWorkspace()
  const project = useProjects(workspace.id).data?.find((item) => item.id === projectId)
  const [filter, setFilter] = useState<PullFilter>('open')
  const [search, setSearch] = useState('')
  const query = useDebouncedValue(search.trim(), 300)
  // pages already shown stay; "Load more" adds the next one
  const [pages, setPages] = useState<string[]>([])
  const first = usePullList(workspace.id, projectId, filter, query, null)
  const shown = FILTERS.find((item) => item.value === filter) ?? FILTERS[0]
  const changeFilter = (value: PullFilter) => { setFilter(value); setPages([]) }

  return <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><Pane>
    <PaneHeader className="max-[899px]:flex-wrap">
      <PaneTitle className="min-w-0 flex-1">{project ? `${project.name} · Pull requests` : 'Pull requests'}</PaneTitle>
      <TaskSearchBox value={search} onChange={(value) => { setSearch(value); setPages([]) }} label="Search pull requests" />
    </PaneHeader>
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex w-full max-w-4xl flex-col gap-3 px-4 py-4">
        <Tabs value={filter} onValueChange={(value) => changeFilter(value as PullFilter)}>
          <TabsList>{FILTERS.map((item) => <TabsTrigger key={item.value} value={item.value}>{item.label}</TabsTrigger>)}</TabsList>
        </Tabs>
        {needsGithubConnection(first.error) ? <ConnectGithubPrompt workspaceId={workspace.id} />
          : first.isError ? <EmptyState icon={MessageText} title="Pull requests unavailable" description="GitHub did not answer. Try again." action={<Button variant="outline" onClick={() => void first.refetch()}>Retry</Button>} />
          : first.data === undefined ? <p className="px-2 py-6 text-sm text-muted-foreground">Loading pull requests…</p>
          : first.data.items.length === 0 ? <EmptyState icon={MessageText} title={query ? 'No pull requests match' : shown.empty} description={query ? 'Try other words.' : 'GitHub has nothing here for this repository.'} />
          : <ul aria-label="Pull requests" className="flex flex-col divide-y rounded-lg border">
            {first.data.items.map((item) => <PullRow key={`${item.repository}#${item.number}`} item={item} />)}
            {pages.map((cursor) => <MorePulls key={cursor} workspaceId={workspace.id} projectId={projectId} filter={filter} search={query} cursor={cursor} />)}
          </ul>}
        <LoadMore workspaceId={workspace.id} projectId={projectId} filter={filter} search={query} cursor={pages.at(-1) ?? null} firstNext={first.data?.next_cursor ?? null} onMore={(cursor) => setPages((all) => [...all, cursor])} />
      </div>
    </div>
  </Pane></div>
}

function PullRow({ item }: { item: GithubPullSummary }) {
  const [owner, repo] = item.repository.split('/')
  return <li>
    <PullLink pullRef={{ owner, repo, number: item.number }} className="flex min-w-0 items-center gap-3 px-3 py-2.5 transition-colors duration-150 ease-out hover:bg-muted/60">
      <PullStateBadge state={item.state} />
      <span className="min-w-0 flex-1 truncate text-sm font-medium text-foreground">{item.title}</span>
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">#{item.number}</span>
      {item.comments > 0 ? <span className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground tabular-nums"><MessageText className="size-3.5" />{item.comments}</span> : null}
      <span className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground"><GithubAvatar actor={item.author} size={18} />{relativeTime(new Date(item.updated_at).toISOString())}</span>
    </PullLink>
  </li>
}

function MorePulls({ workspaceId, projectId, filter, search, cursor }: { workspaceId: string; projectId: string; filter: PullFilter; search: string; cursor: string }) {
  const page = usePullList(workspaceId, projectId, filter, search, cursor)
  return <>{page.data?.items.map((item) => <PullRow key={`${item.repository}#${item.number}`} item={item} />)}</>
}

/** "Load more" for the last page shown (the first page, or the last one added). */
function LoadMore({ workspaceId, projectId, filter, search, cursor, firstNext, onMore }: { workspaceId: string; projectId: string; filter: PullFilter; search: string; cursor: string | null; firstNext: string | null; onMore: (cursor: string) => void }) {
  const last = usePullList(workspaceId, projectId, filter, search, cursor)
  const next = cursor === null ? firstNext : last.data?.next_cursor ?? null
  if (!next) return null
  return <div className="flex items-center gap-2">
    <Button variant="outline" disabled={last.isFetching} onClick={() => onMore(next)}>Load more</Button>
  </div>
}
