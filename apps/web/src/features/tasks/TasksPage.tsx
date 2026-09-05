import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { Add, ChevronDown, TaskSquare } from 'reicon-react'
import { Dropdown } from '../../components/ui/Dropdown'
import { EmptyState } from '../../components/ui/EmptyState'
import { useCurrentUser } from '../auth/api'
import { useMembers } from '../workspaces/api'
import { useWorkspace } from '../workspaces/workspaceContext'
import { useAllStatuses, useProjects } from './api/projects'
import { useLabels } from './api/labels'
import { taskFromRecord } from './api/models'
import {
  useCommentAttachments,
  useCreateTask,
  useTask,
  useTaskActivity,
  useTaskAttachments,
  useTaskComments,
  useTasks,
} from './api/tasks'
import { ProjectRail } from './components/ProjectRail'
import { TaskBoard } from './components/TaskBoard'
import { TaskDetail } from './components/TaskDetail'
import { TaskFilters } from './components/TaskFilters'
import { TaskList } from './components/TaskList'
import { filterTasks, needsExhaustiveTaskList, resolveStatusId, statusGroups, taskApiSort, type SortKey } from './tasksLib'
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
  const createTask = useCreateTask(workspace.id)

  const [layout, setLayout] = useState<'list' | 'board'>(() => searchParams.get('layout') === 'board' ? 'board' : 'list')
  const [sort, setSort] = useState<SortKey>('manual')
  const [statusFilter, setStatusFilter] = useState<string | null>(null)
  const [assigneeFilter, setAssigneeFilter] = useState<string | null>(null)
  const searchFilter = searchParams.get('q') ?? ''
  const projectFilter = searchParams.get('project')
  const apiStatus = projectFilter ? resolveStatusId(statusesQuery.data, projectFilter, statusFilter) : undefined
  const exhaustiveStatusFilter = needsExhaustiveTaskList(projectFilter, statusFilter)
  const tasksQuery = useTasks(workspace.id, {
    project_id: projectFilter ?? undefined,
    status_id: statusFilter ? apiStatus : undefined,
    assignee_id: assigneeFilter ?? undefined,
    search: searchFilter || undefined,
    ...taskApiSort(sort),
    limit: 50,
  }, exhaustiveStatusFilter)
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
  persisted.delete('layout')
  const search = persisted.toString()
  const searchSuffix = search ? `?${search}` : ''

  const setProjectFilter = (projectId: string | null) => {
    const next = new URLSearchParams(searchParams)
    if (projectId) next.set('project', projectId)
    else next.delete('project')
    next.delete('new')
    if (taskId) navigate(`/tasks${next.size > 0 ? `?${next}` : ''}`)
    else setSearchParams(next, { replace: true })
  }
  const openTask = (id: string) => navigate(`/tasks/${id}${searchSuffix}`)
  const closeTask = () => navigate(`/tasks${searchSuffix}`)
  const setSearchFilter = (value: string) => {
    const next = new URLSearchParams(searchParams)
    if (value) next.set('q', value)
    else next.delete('q')
    setSearchParams(next, { replace: true })
  }

  const creating = useRef(false)
  const startNewTask = async (statusKey: string | null = null, replace = false) => {
    const projectId = projectFilter ?? projects[0]?.id
    const statusId = projectId ? resolveStatusId(statusesQuery.data, projectId, statusKey) : undefined
    if (!projectId || !statusId || creating.current) return
    creating.current = true
    try {
      const task = await createTask.mutateAsync({ title: 'Untitled', project_id: projectId, status_id: statusId })
      navigate(`/tasks/${task.id}${searchSuffix}`, { replace })
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
  const visibleTasks = filterTasks(tasks, {
    currentUserId: state.currentUserId,
    projectId: projectFilter,
    statusKey: statusFilter,
    assigneeId: assigneeFilter,
    statuses: statusesQuery.data,
  })

  if (projectsQuery.isPending || statusesQuery.isPending || membersQuery.isPending || labelsQuery.isPending || tasksQuery.isPending || (taskId && (detailQuery.isPending || activityQuery.isPending))) {
    return <TaskBoundary title="Loading tasks" description="Loading persisted workspace tasks." />
  }
  if (projectsQuery.isError || statusesQuery.isError || membersQuery.isError || labelsQuery.isError || tasksQuery.isError || detailQuery.isError || commentsQuery.isError || activityQuery.isError || attachmentsQuery.isError || commentAttachments.isError) {
    return <TaskBoundary title="Tasks unavailable" description="The server could not load this workspace. No mock data was substituted." />
  }

  return (
    <div className="page tasks-page" data-view={taskId ? 'detail' : 'list'}>
      <ProjectRail projects={projects} projectId={projectFilter} onSelect={setProjectFilter} />
      {taskId ? (
        <TaskDetail key={taskId} task={activeTask} project={projects.find((project) => project.id === activeTask?.projectId)} state={state} onBack={closeTask} />
      ) : (
        <section className="pane tasks-list-pane">
          <div className="pane-header">
            <Dropdown className="tasks-project-picker" trigger={(open) => (
              <button type="button" className="tasks-project-trigger" data-open={open || undefined}>
                {activeProject ? <span className="pill-dot" style={{ background: activeProject.color }} /> : <TaskSquare size={15} />}
                <span className="pane-title">{activeProject?.name ?? 'All tasks'}</span><ChevronDown size={14} />
              </button>
            )}>
              {(close) => <><button className="popover-option" data-active={projectFilter === null || undefined} onClick={() => { setProjectFilter(null); close() }}><TaskSquare size={15} />All tasks</button>
                {projects.map((project) => <button key={project.id} className="popover-option" data-active={project.id === projectFilter || undefined} onClick={() => { setProjectFilter(project.id); close() }}><span className="pill-dot" style={{ background: project.color }} />{project.name}</button>)}</>}
            </Dropdown>
            <div className="spacer" />
            <TaskFilters users={users} groups={groups} statusKey={statusFilter} assigneeId={assigneeFilter} sort={sort} layout={layout} search={searchFilter} onSearchChange={setSearchFilter} onStatusChange={setStatusFilter} onAssigneeChange={setAssigneeFilter} onSortChange={setSort} onLayoutChange={setLayout} />
            <button className="button button-primary" aria-label="New task" disabled={createTask.isPending} onClick={() => void startNewTask()}><Add size={16} /><span className="tasks-new-label">New task</span></button>
            {createTask.isError ? <span role="alert" className="text-danger text-xs">Task creation failed.</span> : null}
          </div>
          <div className="pane-body">
            {layout === 'board'
              ? <TaskBoard tasks={visibleTasks} users={users} labels={labelsQuery.data} statuses={statusesQuery.data} groups={groups} sort={sort} activeTaskId={null} onOpen={openTask} />
              : <TaskList tasks={visibleTasks} users={users} labels={labelsQuery.data} statuses={statusesQuery.data} groups={groups} sort={sort} onOpen={openTask} onAdd={(key) => void startNewTask(key)} />}
            {tasksQuery.hasNextPage ? <div className="tasks-load-more"><button className="button" disabled={tasksQuery.isFetchingNextPage} onClick={() => void tasksQuery.fetchNextPage()}>{tasksQuery.isFetchingNextPage ? 'Loading…' : 'Load more'}</button></div> : null}
          </div>
        </section>
      )}
    </div>
  )
}

function TaskBoundary({ title, description }: { title: string; description: string }) {
  return <div className="page"><section className="pane" style={{ flex: 1 }}><EmptyState icon={TaskSquare} title={title} description={description} /></section></div>
}
