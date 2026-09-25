import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useLocation, useNavigate, useParams, useSearchParams } from 'react-router'
import { ChevronDown, Menu, Add as Plus, Setting2 as Settings, TaskSquare as SquareCheck } from 'reicon-react'
import { ApiProblem } from '@/api/problem'
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
} from '@/features/tasks/api/tasks'
import { TaskBoard } from '@/features/tasks/components/TaskBoard'
import { TaskDetail } from '@/features/tasks/components/TaskDetail'
import { TaskFilters } from '@/features/tasks/components/TaskFilters'
import { TaskList } from '@/features/tasks/components/TaskList'
import { NewProjectModal } from '@/features/tasks/components/NewProjectModal'
import { taskUnavailableDescription } from '@/features/tasks/taskAvailability'
import { quickSearchTasks, resolveStatusId } from '@/features/tasks/tasksLib'
import { TaskTimeline, type TimelineHandle } from '@/features/tasks/timeline/TaskTimeline'
import { TimelineControls } from '@/features/tasks/timeline/TimelineControls'
import { useTimelineZoom } from '@/features/tasks/timeline/useTimelineZoom'
import { taskRedirect } from '@/features/tasks/taskNavigation'
import { useTaskQuery } from '@/features/views/api/taskQuery'
import { useSavedView, useViewPreference } from '@/features/views/api/views'
import { createDefaultsFromFilter, type GroupContext } from '@/features/views/grouping'
import { groupCreateFields, type GroupValues } from '@/features/views/layoutGroups'
import { AdvancedFilterDialog } from '@/features/views/components/AdvancedFilterDialog'
import { FilterBar, FilterButton } from '@/features/views/components/FilterBar'
import { PRESET_LABEL, type FilterOptions } from '@/features/views/filterFields'
import { legacyFilterValues, withLegacySort } from '@/features/views/legacyFilterAdapter'
import { useViewState, type ViewSource } from '@/features/views/useViewState'
import { validateFilterOnServer } from '@/features/views/validateFilter'
import { isTaskPreset, pageKeyFor, type TaskPreset } from '@/features/views/viewState'

const EMPTY_PROJECTS: NonNullable<ReturnType<typeof useProjects>['data']> = []

const OPTION =
  `group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-[active]:bg-accent data-[active]:font-medium`

/** History state of a task URL: the project page it was opened from, where closing returns. */
type TaskOrigin = { originProject: string | null }

const PRESET_TITLE: Record<TaskPreset, string> = {
  mine: 'My tasks',
  overdue: 'Overdue',
  due_soon: 'Due soon',
  current_week: 'This week',
  my_week: 'My week',
}

export function TasksPage() {
  const { workspace } = useWorkspace()
  return <WorkspaceTasksPage key={workspace.id} />
}

function WorkspaceTasksPage() {
  const { workspace } = useWorkspace()
  const { taskId, viewId } = useParams()
  const navigate = useNavigate()
  const location = useLocation()
  const [searchParams, setSearchParams] = useSearchParams()
  const projectsQuery = useProjects(workspace.id)
  const projects = projectsQuery.data ?? EMPTY_PROJECTS
  const statusesQuery = useAllStatuses(workspace.id, projects)
  const membersQuery = useMembers(workspace.id)
  const labelsQuery = useLabels(workspace.id)
  const currentUser = useCurrentUser()
  const createTask = useCreateTask(workspace.id)
  const [showNewProject, setShowNewProject] = useState(false)
  const [pxPerDay, setPxPerDay] = useTimelineZoom()
  const timelineRef = useRef<TimelineHandle>(null)

  // A saved view (`/views/:viewId`) or a page (`/tasks`, `?project=`, `?view=<preset>`) drives filters and display.
  const basePath = viewId ? `/views/${viewId}` : '/tasks'
  const projectFilter = viewId ? null : searchParams.get('project')
  const presetParam = searchParams.get('view')
  const preset: TaskPreset | null = !viewId && isTaskPreset(presetParam) ? presetParam : null
  const source = useMemo<ViewSource>(() => viewId
    ? { kind: 'view', viewId }
    : { kind: 'page', pageKey: pageKeyFor({ projectId: projectFilter, preset }), preset, projectId: projectFilter },
  [viewId, projectFilter, preset])
  const viewState = useViewState(workspace.id, source)
  const savedView = useSavedView(workspace.id, viewId)
  // the same query useViewState reads: a page stays loading after a failed load, so the error shows from here
  const preference = useViewPreference(workspace.id, source.kind === 'page' ? source.pageKey : 'all', source.kind === 'page')
  // a saved view that failed to load has no state: never run the task query with defaults in its place
  const viewUnavailable = source.kind === 'view' && !viewState.isLoading && viewState.view === undefined
  const { filter, display } = viewState.state
  const layout = display.layout
  const tasksQuery = useTaskQuery(workspace.id, viewState.effective, display, !viewState.isLoading && !viewUnavailable)

  // Quick search is local and never saved (spec §3); another preset or view starts with an empty box.
  const [search, setSearch] = useState('')
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const scopeKey = viewId ? `view:${viewId}` : `preset:${preset ?? 'all'}`
  const lastScope = useRef(scopeKey)
  useEffect(() => {
    if (lastScope.current !== scopeKey) {
      setSearch('')
      lastScope.current = scopeKey
    }
  }, [scopeKey])

  const legacy = legacyFilterValues(filter, display)

  const detailQuery = useTask(workspace.id, taskId)
  const commentsQuery = useTaskComments(workspace.id, taskId)
  const activityQuery = useTaskActivity(workspace.id, taskId)
  const attachmentsQuery = useTaskAttachments(workspace.id, taskId)
  const commentAttachments = useCommentAttachments(workspace.id, taskId, commentsQuery.data ?? [])

  const records = tasksQuery.tasks
  const tasks = useMemo(() => records.map((record) => taskFromRecord(record, projects.find((project) => project.id === record.project_id))), [projects, records])
  const visibleTasks = useMemo(() => quickSearchTasks(tasks, search), [tasks, search])
  const activeTask = detailQuery.data
    ? taskFromRecord(detailQuery.data, projects.find((project) => project.id === detailQuery.data?.project_id), commentsQuery.data, [...(attachmentsQuery.data ?? []), ...commentAttachments.data], activityQuery.data, projects)
    : undefined
  const users = membersQuery.data ?? []
  const filterOptions: FilterOptions = {
    statuses: statusesQuery.data,
    members: users,
    labels: labelsQuery.data ?? [],
    projects,
    currentUserId: currentUser.data?.id ?? '',
  }
  const presetLabel = source.kind === 'page' && source.preset ? PRESET_LABEL[source.preset] : null
  const state = {
    currentUserId: currentUser.data?.id ?? '',
    users,
    statuses: statusesQuery.data,
    labels: labelsQuery.data ?? [],
    tasks,
  }
  // Grouping and new-task defaults see only the page's own workflow: a project page knows only its project and its statuses.
  const groupContext: GroupContext = {
    statuses: projectFilter ? statusesQuery.data.filter((status) => status.projectId === projectFilter) : statusesQuery.data,
    members: users,
    labels: labelsQuery.data ?? [],
    projects: projectFilter ? projects.filter((project) => project.id === projectFilter) : projects,
    currentUserId: state.currentUserId,
    showEmpty: display.show_empty_groups,
  }
  // collapsed groups are remembered per page (not part of the view state)
  const collapseScope = source.kind === 'view' ? `view:${source.viewId}` : source.pageKey

  const persisted = new URLSearchParams(searchParams)
  persisted.delete('new')
  const detailParams = new URLSearchParams(persisted)
  detailParams.delete('project')
  const detailSearch = detailParams.toString()
  const detailSearchSuffix = detailSearch ? `?${detailSearch}` : ''

  // Closing a task returns to the page it was opened from: a list page passes its project along in the
  // history state, and related tasks opened from the detail keep it. Without one (a link) it is `/tasks`.
  const originProject = viewId ? null : taskId ? (location.state as TaskOrigin | null)?.originProject ?? null : projectFilter
  const originState: TaskOrigin = { originProject }
  const closeParams = new URLSearchParams(detailParams)
  closeParams.delete('redirect')
  if (originProject) closeParams.set('project', originProject)
  const closeSearch = closeParams.toString()
  const closeSearchSuffix = closeSearch ? `?${closeSearch}` : ''
  const redirect = taskRedirect(searchParams)

  const setProjectFilter = (projectId: string | null) => {
    const next = new URLSearchParams(searchParams)
    if (projectId) next.set('project', projectId)
    else next.delete('project')
    next.delete('new')
    if (taskId || viewId) navigate(`/tasks${next.size > 0 ? `?${next}` : ''}`)
    else setSearchParams(next, { replace: true })
  }
  const openTask = (id: string) => navigate(`${basePath}/${id}${detailSearchSuffix}`, { state: originState })
  const closeTask = () => navigate(redirect ?? `${basePath}${closeSearchSuffix}`)

  useEffect(() => {
    if (!taskId || !searchParams.has('project')) return
    navigate(`${basePath}/${taskId}${detailSearchSuffix}`, { replace: true, state: { originProject: viewId ? null : searchParams.get('project') } satisfies TaskOrigin })
  }, [basePath, detailSearchSuffix, navigate, searchParams, taskId, viewId])

  useEffect(() => {
    if (!taskId) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') navigate(redirect ?? `${basePath}${closeSearchSuffix}`)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [basePath, closeSearchSuffix, navigate, redirect, taskId])

  const creating = useRef(false)
  /** `values` = the group (and sub-group) whose + was pressed; each value wins over the filter default for its field. */
  const startNewTask = async (values: GroupValues = [], replace = false) => {
    if (creating.current) return
    const fromGroup = groupCreateFields(values)
    const fromFilter = createDefaultsFromFilter(viewState.effective, {
      ...groupContext,
      targetProjectId: fromGroup.projectId ?? projectFilter ?? projects[0]?.id ?? null,
    })
    // a project group wins; otherwise the filter's single project, then the page or first project
    const projectId = fromGroup.projectId ?? fromFilter.project_id
    if (!projectId) return
    // the filter's status was resolved in the filter's project; only reuse it for that project
    const filterStatusId = fromFilter.project_id === projectId ? fromFilter.status_id : undefined
    const statusId = fromGroup.statusKey
      ? resolveStatusId(statusesQuery.data, projectId, fromGroup.statusKey)
      : filterStatusId ?? resolveStatusId(statusesQuery.data, projectId, null)
    if (!statusId) return
    creating.current = true
    try {
      const task = await createTask.mutateAsync({
        ...fromFilter,
        ...fromGroup.body,
        title: 'Untitled',
        project_id: projectId,
        status_id: statusId,
      })
      navigate(`${basePath}/${task.id}${detailSearchSuffix}`, { replace, state: originState })
    } catch {
      // The mutation exposes the server problem beside the create action.
    } finally {
      creating.current = false
    }
  }

  const wantsNew = searchParams.get('new') === '1'
  const stateLoading = viewState.isLoading
  useEffect(() => {
    // new-task defaults come from the page's filter, so wait until it has loaded
    if (wantsNew && !stateLoading && projects.length > 0 && statusesQuery.data.length > 0) void startNewTask([], true)
    // The URL flag is the one-shot trigger; the ref prevents duplicate in-flight creation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wantsNew, stateLoading, projects.length, statusesQuery.data.length])

  const activeProject = projects.find((project) => project.id === projectFilter)
  const viewTitle = viewId ? savedView.data?.name ?? 'View' : preset ? PRESET_TITLE[preset] : 'All tasks'

  if (viewUnavailable) {
    const missing = savedView.error instanceof ApiProblem && savedView.error.status === 404
    return (
      <TaskBoundary
        title={missing ? 'View not found' : 'View unavailable'}
        description={missing ? 'This view may have been deleted, or you may no longer have access to it.' : 'The server could not load this view.'}
        action={<Button variant="outline" onClick={() => navigate('/views')}>Go to views</Button>}
      />
    )
  }
  if (preference.isError) {
    return <TaskBoundary title="Tasks unavailable" description="The server could not load the settings for this page." />
  }
  // `viewState.isLoading` too: after a page or view switch the disabled task query still shows the previous
  // page's tasks as placeholder data, which must not render under the new page's title and placeholder display
  if (viewState.isLoading || projectsQuery.isPending || statusesQuery.isPending || membersQuery.isPending || labelsQuery.isPending || tasksQuery.isLoading || (taskId && (detailQuery.isPending || activityQuery.isPending))) {
    return <TaskBoundary title="Loading tasks" description="Loading persisted workspace tasks." />
  }
  if (detailQuery.isError) {
    return <TaskBoundary title="Task unavailable" description={taskUnavailableDescription(detailQuery.error)} />
  }
  if (taskId && (commentsQuery.isError || activityQuery.isError || attachmentsQuery.isError || commentAttachments.isError)) {
    return <TaskBoundary title="Task unavailable" description="The server could not load this task." />
  }
  if (projectsQuery.isError || statusesQuery.isError || membersQuery.isError || labelsQuery.isError || tasksQuery.error) {
    return <TaskBoundary title="Tasks unavailable" description="The server could not load this workspace." />
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background" data-view={taskId ? 'detail' : 'list'}>
      {taskId ? (
        <TaskDetail key={taskId} task={activeTask} project={projects.find((project) => project.id === activeTask?.projectId)} state={state} onBack={closeTask} onOpenTask={openTask} />
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
            <FilterButton filter={viewState.state.filter} options={filterOptions} onChange={viewState.setFilter} onOpenAdvanced={() => setAdvancedOpen(true)} />
            <TaskFilters
              sort={legacy.sort}
              layout={layout}
              search={search}
              onSearchChange={setSearch}
              onSortChange={(value) => viewState.setDisplay(withLegacySort(value))}
              onLayoutChange={(value) => viewState.setDisplay({ layout: value })}
            />
            <Button aria-label="New task" className="max-[899px]:w-8 max-[899px]:px-0" disabled={createTask.isPending} onClick={() => void startNewTask()}><Plus className="size-4" /><span className="max-[899px]:hidden">New task</span></Button>
            {createTask.isError ? <span role="alert" className="text-xs text-destructive">Task creation failed.</span> : null}
          </div>
          <FilterBar
            key={source.kind === 'view' ? source.viewId : source.pageKey}
            filter={viewState.state.filter}
            options={filterOptions}
            onChange={viewState.setFilter}
            presetLabel={presetLabel}
            onOpenAdvanced={() => setAdvancedOpen(true)}
          />
          <AdvancedFilterDialog
            open={advancedOpen}
            onOpenChange={setAdvancedOpen}
            filter={viewState.state.filter}
            options={filterOptions}
            onApply={viewState.setFilter}
            validate={(next) => validateFilterOnServer(workspace.id, next)}
          />
          {showNewProject ? <NewProjectModal onClose={() => setShowNewProject(false)} onCreated={(project) => { setProjectFilter(project.id); setShowNewProject(false) }} /> : null}
          <div className={`min-h-0 flex-1 ${layout === 'timeline' ? 'overflow-hidden' : 'overflow-y-auto'}`}>
            {layout === 'timeline'
              ? <TaskTimeline ref={timelineRef} key={workspace.id} tasks={visibleTasks} projects={projects} statuses={statusesQuery.data} users={users} groupBy={display.group_by} groupContext={groupContext} pxPerDay={pxPerDay} onZoomChange={setPxPerDay} onOpen={openTask} />
              : layout === 'board'
                ? <TaskBoard key={`${workspace.id}:${collapseScope}`} tasks={visibleTasks} users={users} labels={labelsQuery.data ?? []} statuses={statusesQuery.data} projects={projects} display={display} groupContext={groupContext} collapseScope={collapseScope} activeTaskId={null} onOpen={openTask} />
                : <TaskList key={`${workspace.id}:${collapseScope}`} tasks={visibleTasks} users={users} labels={labelsQuery.data ?? []} statuses={statusesQuery.data} projects={projects} display={display} groupContext={groupContext} collapseScope={collapseScope} onOpen={openTask} onAdd={(values) => void startNewTask(values)} />}
          </div>
        </section>
      )}
    </div>
  )
}

function TaskBoundary({ title, description, action }: { title: string; description: string; action?: ReactNode }) {
  return <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background"><section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background"><EmptyState icon={SquareCheck} title={title} description={description} action={action} /></section></div>
}
