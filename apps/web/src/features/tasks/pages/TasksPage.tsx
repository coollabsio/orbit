import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { ChevronDown, Menu, Add as Plus, Setting2 as Settings, TaskSquare as SquareCheck } from 'reicon-react'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { EmptyState } from '@/components/common/EmptyState'
import { useCurrentUser } from '@/features/auth/api'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useAllStatuses, useProjects } from '@/features/tasks/api/projects'
import { useLabels } from '@/features/tasks/api/labels'
import { taskFromRecord } from '@/features/tasks/api/models'
import {
  useCommentAttachments,
  useCreateTask,
  useTask,
  useTaskActivity,
  useTaskAttachments,
  useTaskComments,
  useTasks,
} from '@/features/tasks/api/tasks'
import { TaskBoard } from '@/features/tasks/components/TaskBoard'
import { TaskDetail } from '@/features/tasks/components/TaskDetail'
import { TaskFilters } from '@/features/tasks/components/TaskFilters'
import { TaskList } from '@/features/tasks/components/TaskList'
import { NewProjectModal } from '@/features/tasks/components/NewProjectModal'
import { taskUnavailableDescription } from '@/features/tasks/taskAvailability'
import { taskViewCreateDefaults, type TaskView } from '@/features/tasks/taskMeta'
import { filterTasks, resolveLayout, resolveStatusId, statusGroups, taskApiSort, type TaskLayout } from '@/features/tasks/tasksLib'
import { TaskTimeline, type TimelineHandle } from '@/features/tasks/timeline/TaskTimeline'
import { TimelineControls } from '@/features/tasks/timeline/TimelineControls'
import { useTimelineZoom } from '@/features/tasks/timeline/useTimelineZoom'
import { taskRedirect } from '@/features/tasks/taskNavigation'
import { useTaskPreferences } from '@/features/tasks/taskPreferences'

const LAYOUT_KEY = 'orbit:task_layout'
const EMPTY_PROJECTS: NonNullable<ReturnType<typeof useProjects>['data']> = []

const OPTION =
  `group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-[active]:bg-accent data-[active]:font-medium`

export function TasksPage() {
  const { workspace } = useWorkspace()
  return <WorkspaceTasksPage key={workspace.id} />
}

function WorkspaceTasksPage() {
  const { workspace } = useWorkspace()
  const { taskId } = useParams()
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  const projectsQuery = useProjects(workspace.id)
  const projects = projectsQuery.data ?? EMPTY_PROJECTS
  const statusesQuery = useAllStatuses(workspace.id, projects)
  const membersQuery = useMembers(workspace.id)
  const labelsQuery = useLabels(workspace.id)
  const currentUser = useCurrentUser()
  const createTask = useCreateTask(workspace.id)
  const [showNewProject, setShowNewProject] = useState(false)

  const urlLayout = searchParams.get('layout')
  const layout = resolveLayout(urlLayout, localStorage.getItem(LAYOUT_KEY))
  // a layout arriving by URL (shared link, back from a task) is also the one to remember
  useEffect(() => {
    if (urlLayout) localStorage.setItem(LAYOUT_KEY, layout)
  }, [urlLayout, layout])
  const setLayout = (next: TaskLayout) => {
    localStorage.setItem(LAYOUT_KEY, next)
    const params = new URLSearchParams(searchParams)
    if (next === 'list') params.delete('layout')
    else params.set('layout', next)
    setSearchParams(params, { replace: true })
  }
  const [pxPerDay, setPxPerDay] = useTimelineZoom()
  const timelineRef = useRef<TimelineHandle>(null)
  const [preferences, setPreferences] = useTaskPreferences(workspace.id)
  const { sort, statusFilter, assigneeFilter, unassignedFilter, labelFilter, priorityFilter, searchFilter } = preferences
  const setPreference = <K extends keyof typeof preferences>(key: K, value: typeof preferences[K]) => {
    setPreferences((current) => ({ ...current, [key]: value }))
  }
  const projectFilter = searchParams.get('project')
  const viewFilter = ['mine', 'overdue', 'due_soon', 'current_week', 'my_week'].includes(searchParams.get('view') ?? '')
    ? searchParams.get('view') as TaskView
    : undefined

  const lastView = useRef(viewFilter)
  useEffect(() => {
    if (lastView.current !== viewFilter) {
      setPreferences((current) => ({ ...current, searchFilter: '' }))
      lastView.current = viewFilter
    }
  }, [viewFilter, setPreferences])

  const apiStatus = projectFilter ? resolveStatusId(statusesQuery.data, projectFilter, statusFilter) : undefined
  const tasksQuery = useTasks(workspace.id, {
    project_id: projectFilter ?? undefined,
    status_id: statusFilter ? apiStatus : undefined,
    assignee_id: assigneeFilter ?? undefined,
    unassigned: unassignedFilter || undefined,
    label_id: labelFilter ?? undefined,
    priority: priorityFilter ?? undefined,
    view: viewFilter,
    ...taskApiSort(sort),
    limit: 50,
  }, true)
  const detailQuery = useTask(workspace.id, taskId)
  const commentsQuery = useTaskComments(workspace.id, taskId)
  const activityQuery = useTaskActivity(workspace.id, taskId)
  const attachmentsQuery = useTaskAttachments(workspace.id, taskId)
  const commentAttachments = useCommentAttachments(workspace.id, taskId, commentsQuery.data ?? [])

  const records = useMemo(() => tasksQuery.data?.pages.flatMap((page) => page.items) ?? [], [tasksQuery.data])
  const tasks = useMemo(() => records.map((record) => taskFromRecord(record, projects.find((project) => project.id === record.project_id))), [projects, records])
  const activeTask = detailQuery.data
    ? taskFromRecord(detailQuery.data, projects.find((project) => project.id === detailQuery.data?.project_id), commentsQuery.data, [...(attachmentsQuery.data ?? []), ...commentAttachments.data], activityQuery.data)
    : undefined
  const users = membersQuery.data ?? []
  const state = {
    currentUserId: currentUser.data?.id ?? '',
    users,
    statuses: statusesQuery.data,
    labels: labelsQuery.data ?? [],
    tasks,
  }

  const persisted = new URLSearchParams(searchParams)
  persisted.delete('new')
  const detailParams = new URLSearchParams(persisted)
  detailParams.delete('project')
  const detailSearch = detailParams.toString()
  const detailSearchSuffix = detailSearch ? `?${detailSearch}` : ''

  const taskProjectId = activeTask?.projectId
  const closeParams = new URLSearchParams(detailParams)
  closeParams.delete('redirect')
  if (taskProjectId) closeParams.set('project', taskProjectId)
  const closeSearch = closeParams.toString()
  const closeSearchSuffix = closeSearch ? `?${closeSearch}` : ''
  const redirect = taskRedirect(searchParams)

  const setProjectFilter = (projectId: string | null) => {
    const next = new URLSearchParams(searchParams)
    if (projectId) next.set('project', projectId)
    else next.delete('project')
    next.delete('new')
    if (taskId) navigate(`/tasks${next.size > 0 ? `?${next}` : ''}`)
    else setSearchParams(next, { replace: true })
  }
  const openTask = (id: string) => navigate(`/tasks/${id}${detailSearchSuffix}`)
  const closeTask = () => navigate(redirect ?? `/tasks${closeSearchSuffix}`)

  useEffect(() => {
    if (!taskId || !searchParams.has('project')) return
    navigate(`/tasks/${taskId}${detailSearchSuffix}`, { replace: true })
  }, [detailSearchSuffix, navigate, searchParams, taskId])

  useEffect(() => {
    if (!taskId) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') navigate(redirect ?? `/tasks${closeSearchSuffix}`)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [closeSearchSuffix, navigate, redirect, taskId])

  const creating = useRef(false)
  const startNewTask = async (statusKey: string | null = null, replace = false) => {
    const projectId = projectFilter ?? projects[0]?.id
    const statusId = projectId ? resolveStatusId(statusesQuery.data, projectId, statusKey) : undefined
    if (!projectId || !statusId || creating.current) return
    creating.current = true
    try {
      const task = await createTask.mutateAsync({
        title: 'Untitled',
        project_id: projectId,
        status_id: statusId,
        ...taskViewCreateDefaults(viewFilter, state.currentUserId),
      })
      navigate(`/tasks/${task.id}${detailSearchSuffix}`, { replace })
    } catch {
      // The mutation exposes the server problem beside the create action.
    } finally {
      creating.current = false
    }
  }

  const wantsNew = searchParams.get('new') === '1'
  useEffect(() => {
    if (wantsNew && projects.length > 0 && statusesQuery.data.length > 0) void startNewTask(null, true)
    // The URL flag is the one-shot trigger; the ref prevents duplicate in-flight creation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wantsNew, projects.length, statusesQuery.data.length])

  const groups = statusGroups(statusesQuery.data, projectFilter)
  const activeProject = projects.find((project) => project.id === projectFilter)
  const viewTitle = viewFilter === 'mine'
    ? 'My tasks'
    : viewFilter === 'overdue'
      ? 'Overdue'
      : viewFilter === 'due_soon'
        ? 'Due soon'
        : viewFilter === 'current_week'
          ? 'This week'
          : viewFilter === 'my_week'
            ? 'My week'
            : 'All tasks'
  const visibleTasks = filterTasks(tasks, {
    currentUserId: state.currentUserId,
    projectId: projectFilter,
    statusKey: statusFilter,
    assigneeId: assigneeFilter,
    unassigned: unassignedFilter,
    labelId: labelFilter,
    priority: priorityFilter,
    statuses: statusesQuery.data,
    search: searchFilter,
  })

  if (projectsQuery.isPending || statusesQuery.isPending || membersQuery.isPending || labelsQuery.isPending || tasksQuery.isPending || (taskId && (detailQuery.isPending || activityQuery.isPending))) {
    return <TaskBoundary title="Loading tasks" description="Loading persisted workspace tasks." />
  }
  if (detailQuery.isError) {
    return <TaskBoundary title="Task unavailable" description={taskUnavailableDescription(detailQuery.error)} />
  }
  if (taskId && (commentsQuery.isError || activityQuery.isError || attachmentsQuery.isError || commentAttachments.isError)) {
    return <TaskBoundary title="Task unavailable" description="The server could not load this task." />
  }
  if (projectsQuery.isError || statusesQuery.isError || membersQuery.isError || labelsQuery.isError || tasksQuery.isError) {
    return <TaskBoundary title="Tasks unavailable" description="The server could not load this workspace." />
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background" data-view={taskId ? 'detail' : 'list'}>
      {taskId ? (
        <TaskDetail key={taskId} task={activeTask} project={projects.find((project) => project.id === activeTask?.projectId)} state={state} onBack={closeTask} />
      ) : (
        <section className="relative flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
          <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border bg-background px-3 py-2 text-foreground max-[899px]:min-h-11 max-[899px]:flex-wrap max-[899px]:border-b-0 max-[899px]:px-2 max-[899px]:py-1.5">
            <Button type="button" variant="ghost" size="icon-sm" className="hidden shrink-0 text-muted-foreground/70 max-[899px]:inline-flex" aria-label="Menu" onClick={() => window.dispatchEvent(new CustomEvent('open-sidebar'))}>
              <Menu className="size-[18px]" />
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button type="button" variant="ghost" className="h-auto min-w-0 gap-[7px] rounded-md border-0 px-[7px] py-[5px] font-normal text-muted-foreground transition-colors hover:bg-accent hover:text-foreground aria-expanded:bg-accent aria-expanded:text-foreground dark:hover:bg-accent" aria-label="Select project">
                    {activeProject ? <span className="size-1.5 shrink-0 rounded-full" style={{ background: activeProject.color }} /> : null}
                    <span>{activeProject?.name ?? 'All projects'}</span><ChevronDown className="size-3.5" />
                  </Button>
                }
              />
              <DropdownMenuContent className="flex w-auto min-w-[190px] flex-col gap-px p-1">
                <DropdownMenuItem className={OPTION} data-active={projectFilter === null || undefined} onClick={() => setProjectFilter(null)}><SquareCheck className="size-3.5" />All projects</DropdownMenuItem>
                {projects.map((project) => <div key={project.id} className="relative flex items-center">
                  <DropdownMenuItem className={`${OPTION} min-w-0 flex-1 pr-8`} data-active={project.id === projectFilter || undefined} onClick={() => setProjectFilter(project.id)}>
                    <span className="size-1.5 shrink-0 rounded-full" style={{ background: project.color }} />
                    <span className="min-w-0 flex-1 truncate">{project.name}</span>
                  </DropdownMenuItem>
                  <DropdownMenuItem className="absolute right-1 flex size-6 items-center justify-center rounded-md px-0 py-0 text-muted-foreground/70 transition hover:bg-accent hover:text-foreground dark:hover:bg-accent" aria-label={`${project.name} settings`} title="Project settings" onClick={() => navigate(`/tasks/projects/${project.id}/settings`)}><Settings className="size-3.5" /></DropdownMenuItem>
                </div>)}
                <DropdownMenuSeparator className="my-1 shrink-0" />
                <DropdownMenuItem className={OPTION} onClick={() => setShowNewProject(true)}><Plus className="size-3.5" />New project</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <span className="truncate text-[13px] font-semibold text-foreground">{viewTitle}</span>
            <div className="flex-1" />
            {layout === 'timeline' ? <TimelineControls pxPerDay={pxPerDay} onZoomChange={setPxPerDay} onToday={() => timelineRef.current?.scrollToToday()} /> : null}
            <TaskFilters users={users} labels={labelsQuery.data ?? []} groups={groups} statusKey={statusFilter} assigneeId={assigneeFilter} unassigned={unassignedFilter} labelId={labelFilter} priority={priorityFilter} sort={sort} layout={layout} search={searchFilter} onSearchChange={(value) => setPreference('searchFilter', value)} onStatusChange={(value) => setPreference('statusFilter', value)} onAssigneeChange={(value) => setPreference('assigneeFilter', value)} onUnassignedChange={(value) => setPreference('unassignedFilter', value)} onLabelChange={(value) => setPreference('labelFilter', value)} onPriorityChange={(value) => setPreference('priorityFilter', value)} onSortChange={(value) => setPreference('sort', value)} onLayoutChange={setLayout} />
            <Button aria-label="New task" className="max-[899px]:w-8 max-[899px]:px-0" disabled={createTask.isPending} onClick={() => void startNewTask()}><Plus className="size-4" /><span className="max-[899px]:hidden">New task</span></Button>
            {createTask.isError ? <span role="alert" className="text-xs text-destructive">Task creation failed.</span> : null}
          </div>
          {showNewProject ? <NewProjectModal onClose={() => setShowNewProject(false)} onCreated={(project) => { setProjectFilter(project.id); setShowNewProject(false) }} /> : null}
          <div className={`min-h-0 flex-1 ${layout === 'timeline' ? 'overflow-hidden' : 'overflow-y-auto'}`}>
            {layout === 'timeline'
              ? <TaskTimeline ref={timelineRef} key={workspace.id} tasks={visibleTasks} projects={projects} statuses={statusesQuery.data} users={users} grouped={!projectFilter} pxPerDay={pxPerDay} onZoomChange={setPxPerDay} onOpen={openTask} />
              : layout === 'board'
                ? <TaskBoard tasks={visibleTasks} users={users} labels={labelsQuery.data} statuses={statusesQuery.data} groups={groups} sort={sort} activeTaskId={null} onOpen={openTask} />
                : <TaskList key={workspace.id} tasks={visibleTasks} users={users} labels={labelsQuery.data} statuses={statusesQuery.data} groups={groups} sort={sort} onOpen={openTask} onAdd={(key) => void startNewTask(key)} />}
            {tasksQuery.hasNextPage ? <div className="flex justify-center p-4"><Button variant="outline" disabled={tasksQuery.isFetchingNextPage} onClick={() => void tasksQuery.fetchNextPage()}>{tasksQuery.isFetchingNextPage ? 'Loading…' : 'Load more'}</Button></div> : null}
          </div>
        </section>
      )}
    </div>
  )
}

function TaskBoundary({ title, description }: { title: string; description: string }) {
  return <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background"><EmptyState icon={SquareCheck} title={title} description={description} /></section></div>
}
