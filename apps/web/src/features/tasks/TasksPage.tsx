import { useEffect, useMemo, useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { Add, ChevronDown, Setting2, TaskSquare } from 'reicon-react'
import { Dropdown } from '../../components/ui/Dropdown'
import { TopbarSlot } from '../../components/shell/TopbarSlot'
import { EmptyState } from '../../components/ui/EmptyState'
import { useCurrentUser } from '../auth/api'
import { useMembers } from '../workspaces/api'
import { useWorkspace } from '../workspaces/workspaceContext'
import { useAllStatuses, useProjects } from './api/projects'
import { projectSettingsLabel, projectSettingsPath } from './api/projectDraft'
import { useLabels } from './api/labels'
import { taskFromRecord } from './api/models'
import {
  useCommentAttachments,
  useTask,
  useTaskActivity,
  useTaskAttachments,
  useTaskComments,
  useTasks,
} from './api/tasks'
import { prefetchRichTextEditor } from '../../components/editor/RichTextEditor'
import { TaskBoard } from './components/TaskBoard'
import { TaskDetail } from './components/TaskDetail'
import { TaskFilters } from './components/TaskFilters'
import { TaskList } from './components/TaskList'
import { shouldCloseTaskOnKey } from './closeOnEscape'
import { NewProjectModal } from './components/NewProjectModal'
import { NewTaskModal } from './components/NewTaskModal'
import { filterTasks, resolveStatusId, statusGroups, taskApiSort, type SortKey } from './tasksLib'
import './tasks.css'

const EMPTY_PROJECTS: NonNullable<ReturnType<typeof useProjects>['data']> = []

export function TasksPage() {
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
  const [showNewProject, setShowNewProject] = useState(false)
  // Non-null while the create-issue modal is open; carries a preselected status key
  // from a list group "+", else null.
  const [newTask, setNewTask] = useState<{ statusKey: string | null } | null>(null)

  const [layout, setLayout] = useState<'list' | 'board'>(() => searchParams.get('layout') === 'board' ? 'board' : 'list')
  const [sort, setSort] = useState<SortKey>('manual')
  const [statusFilter, setStatusFilter] = useState<string | null>(null)
  const [assigneeFilter, setAssigneeFilter] = useState<string | null>(null)
  const [searchFilter, setSearchFilter] = useState('')
  // Sub-issues are hidden from the list by default; the Display menu nests them.
  const [showSubIssues, setShowSubIssues] = useState(false)
  const projectFilter = searchParams.get('project')
  const viewFilter = ['mine', 'overdue', 'due_soon', 'current_week'].includes(searchParams.get('view') ?? '')
    ? searchParams.get('view') ?? undefined
    : undefined
  const apiStatus = projectFilter ? resolveStatusId(statusesQuery.data, projectFilter, statusFilter) : undefined
  const tasksQuery = useTasks(workspace.id, {
    project_id: projectFilter ?? undefined,
    status_id: statusFilter ? apiStatus : undefined,
    assignee_id: assigneeFilter ?? undefined,
    view: viewFilter,
    nesting: showSubIssues ? 'all' : 'roots',
    ...taskApiSort(sort),
    limit: 50,
  }, true)
  const detailQuery = useTask(workspace.id, taskId)
  const commentsQuery = useTaskComments(workspace.id, taskId)
  const activityQuery = useTaskActivity(workspace.id, taskId)
  const attachmentsQuery = useTaskAttachments(workspace.id, taskId)
  const commentAttachments = useCommentAttachments(workspace.id, taskId, commentsQuery.data ?? [])

  const records = useMemo(() => tasksQuery.data?.pages.flatMap((page) => page.items) ?? [], [tasksQuery.data])
  const tasks = useMemo(() => records.map((record) => taskFromRecord(record)), [records])
  const activeTask = detailQuery.data
    ? taskFromRecord(detailQuery.data, commentsQuery.data, [...(attachmentsQuery.data ?? []), ...commentAttachments.data], activityQuery.data)
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
  persisted.delete('layout')
  const detailParams = new URLSearchParams(persisted)
  detailParams.delete('project')
  const detailSearch = detailParams.toString()
  const detailSearchSuffix = detailSearch ? `?${detailSearch}` : ''

  const taskProjectId = activeTask?.projectId
  const closeParams = new URLSearchParams(detailParams)
  if (taskProjectId) closeParams.set('project', taskProjectId)
  const closeSearch = closeParams.toString()
  const closeSearchSuffix = closeSearch ? `?${closeSearch}` : ''

  const setProjectFilter = (projectId: string | null) => {
    const next = new URLSearchParams(searchParams)
    if (projectId) next.set('project', projectId)
    else next.delete('project')
    next.delete('new')
    if (taskId) navigate(`/tasks${next.size > 0 ? `?${next}` : ''}`)
    else setSearchParams(next, { replace: true })
  }
  const openTask = (id: string) => navigate(`/tasks/${id}${detailSearchSuffix}`)
  const closeTask = () => navigate(`/tasks${closeSearchSuffix}`)

  useEffect(() => {
    if (!taskId || !searchParams.has('project')) return
    navigate(`/tasks/${taskId}${detailSearchSuffix}`, { replace: true })
  }, [detailSearchSuffix, navigate, searchParams, taskId])

  useEffect(() => {
    if (!taskId) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (shouldCloseTaskOnKey(event)) navigate(`/tasks${closeSearchSuffix}`)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [closeSearchSuffix, navigate, taskId])

  // Warm the heavy editor chunk while the page is idle, so opening the create modal
  // (or clicking a description) does not stall on the first TipTap/ProseMirror import.
  useEffect(() => {
    const idle = window.requestIdleCallback
    if (idle) {
      const handle = idle(() => prefetchRichTextEditor())
      return () => window.cancelIdleCallback?.(handle)
    }
    const timer = setTimeout(() => prefetchRichTextEditor(), 300)
    return () => clearTimeout(timer)
  }, [])

  // Creation happens entirely in the modal, so the user never navigates into the
  // full-page detail just to fill an issue in.
  const openNewTask = (statusKey: string | null = null) => setNewTask({ statusKey })

  const wantsNew = searchParams.get('new') === '1'
  useEffect(() => {
    if (!wantsNew) return
    // The `c` shortcut and topbar route here with `?new=1`; open the modal once and
    // clear the flag so a refresh does not reopen it.
    setNewTask((current) => current ?? { statusKey: null })
    const next = new URLSearchParams(searchParams)
    next.delete('new')
    setSearchParams(next, { replace: true })
  }, [wantsNew, searchParams, setSearchParams])

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
          : 'All tasks'
  const visibleTasks = filterTasks(tasks, {
    currentUserId: state.currentUserId,
    projectId: projectFilter,
    statusKey: statusFilter,
    assigneeId: assigneeFilter,
    statuses: statusesQuery.data,
    search: searchFilter,
  })

  if (projectsQuery.isPending || statusesQuery.isPending || membersQuery.isPending || labelsQuery.isPending || tasksQuery.isPending || (taskId && (detailQuery.isPending || activityQuery.isPending))) {
    return <TaskBoundary title="Loading tasks" description="Loading persisted workspace tasks." />
  }
  if (projectsQuery.isError || statusesQuery.isError || membersQuery.isError || labelsQuery.isError || tasksQuery.isError || detailQuery.isError || commentsQuery.isError || activityQuery.isError || attachmentsQuery.isError || commentAttachments.isError) {
    return <TaskBoundary title="Tasks unavailable" description="The server could not load this workspace. No mock data was substituted." />
  }

  return (
    <div className="page tasks-page" data-view={taskId ? 'detail' : 'list'}>
      {taskId ? (
        <TaskDetail key={taskId} task={activeTask} project={projects.find((project) => project.id === activeTask?.projectId)} projects={projects} state={state} onBack={closeTask} onOpenTask={openTask} />
      ) : (
        <section className="pane tasks-list-pane">
          <TopbarSlot side="left">
            <span className="topbar-crumb-sep" aria-hidden="true">›</span>
            <Dropdown className="tasks-project-picker" trigger={(open) => (
              <button type="button" className="tasks-project-trigger" data-open={open || undefined} aria-label="Select project">
                {activeProject ? <span className="pill-dot" style={{ background: activeProject.color }} /> : null}
                <span className="pane-title">{activeProject?.name ?? 'All projects'}</span><ChevronDown size={14} />
              </button>
            )}>
              {(close) => <><button className="popover-option" data-active={projectFilter === null || undefined} onClick={() => { setProjectFilter(null); close() }}><TaskSquare size={15} />All projects</button>
                {projects.map((project) => <button key={project.id} className="popover-option" data-active={project.id === projectFilter || undefined} onClick={() => { setProjectFilter(project.id); close() }}><span className="pill-dot" style={{ background: project.color }} />{project.name}</button>)}
                <div className="popover-separator" />
                <button className="popover-option" onClick={() => { close(); setShowNewProject(true) }}><Add size={15} />New project</button>
                {activeProject ? <button className="popover-option" onClick={() => {
                  const path = projectSettingsPath(activeProject.id)
                  close()
                  if (path) navigate(path)
                }}><Setting2 size={15} />{projectSettingsLabel()}</button> : null}
                </>}
            </Dropdown>
            <span className="topbar-crumb-sep" aria-hidden="true">›</span>
            <span className="topbar-crumb" data-current="true">{viewTitle}</span>
          </TopbarSlot>
          <TopbarSlot side="right">
            <TaskFilters users={users} groups={groups} statusKey={statusFilter} assigneeId={assigneeFilter} sort={sort} layout={layout} search={searchFilter} onSearchChange={setSearchFilter} onStatusChange={setStatusFilter} onAssigneeChange={setAssigneeFilter} onSortChange={setSort} onLayoutChange={setLayout} showSubIssues={showSubIssues} onShowSubIssuesChange={setShowSubIssues} />
            <button className="button button-primary" aria-label="New task" onClick={() => openNewTask()}><Add size={16} /><span className="tasks-new-label">New task</span></button>
          </TopbarSlot>
          {showNewProject ? <NewProjectModal onClose={() => setShowNewProject(false)} onCreated={(project) => { setProjectFilter(project.id); setShowNewProject(false) }} /> : null}
          <div className="pane-body">
            {layout === 'board'
              ? <TaskBoard tasks={visibleTasks} users={users} labels={labelsQuery.data} statuses={statusesQuery.data} groups={groups} sort={sort} activeTaskId={null} onOpen={openTask} />
              : <TaskList key={workspace.id} tasks={visibleTasks} users={users} labels={labelsQuery.data} statuses={statusesQuery.data} groups={groups} sort={sort} onOpen={openTask} onAdd={(key) => openNewTask(key)} showSubIssues={showSubIssues} />}
            {tasksQuery.hasNextPage ? <div className="tasks-load-more"><button className="button" disabled={tasksQuery.isFetchingNextPage} onClick={() => void tasksQuery.fetchNextPage()}>{tasksQuery.isFetchingNextPage ? 'Loading…' : 'Load more'}</button></div> : null}
          </div>
        </section>
      )}
      {newTask ? (
        <NewTaskModal
          workspaceId={workspace.id}
          projects={projects}
          statuses={statusesQuery.data}
          users={users}
          labels={labelsQuery.data ?? []}
          defaultProjectId={projectFilter}
          defaultStatusKey={newTask.statusKey}
          onClose={() => setNewTask(null)}
        />
      ) : null}
    </div>
  )
}

function TaskBoundary({ title, description }: { title: string; description: string }) {
  return <div className="page"><section className="pane" style={{ flex: 1 }}><EmptyState icon={TaskSquare} title={title} description={description} /></section></div>
}
