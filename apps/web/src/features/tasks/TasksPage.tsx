import { useEffect, useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { Add } from 'reicon-react'
import { createTask } from '../../mock/actions'
import { useAppState } from '../../mock/store'
import { ProjectRail } from './components/ProjectRail'
import { TaskBoard } from './components/TaskBoard'
import { TaskDetail } from './components/TaskDetail'
import { TaskFilters } from './components/TaskFilters'
import { TaskList } from './components/TaskList'
import { filterTasks, resolveStatusId, statusGroups, type SortKey } from './tasksLib'
import './tasks.css'

export function TasksPage() {
  const { taskId } = useParams()
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  const state = useAppState()

  // ?layout=board opens the kanban directly (deep link); the Display menu changes it afterwards
  const [layout, setLayout] = useState<'list' | 'board'>(() => (searchParams.get('layout') === 'board' ? 'board' : 'list'))
  const [sort, setSort] = useState<SortKey>('manual')
  const [statusFilter, setStatusFilter] = useState<string | null>(null)
  const [assigneeFilter, setAssigneeFilter] = useState<string | null>(null)
  const projectFilter = searchParams.get('project')
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
    // picking a project in the rail also leaves an open task (back to the list)
    if (taskId) navigate(`/tasks${next.size > 0 ? `?${next}` : ''}`)
    else setSearchParams(next, { replace: true })
  }

  const openTask = (id: string) => navigate(`/tasks/${id}${searchSuffix}`)
  const closeTask = () => navigate(`/tasks${searchSuffix}`)

  // "New task" (header, a group's "+", or the topbar's ?new=1) creates an empty task and opens it;
  // the task view focuses the title field.
  const startNewTask = (statusKey: string | null = null, replace = false) => {
    const projectId = projectFilter ?? state.projects[0]?.id
    if (!projectId) return
    const task = createTask({ title: '', projectId, statusId: resolveStatusId(state.statuses, projectId, statusKey) })
    navigate(`/tasks/${task.id}${searchSuffix}`, { replace })
  }

  const wantsNew = searchParams.get('new') === '1'
  useEffect(() => {
    if (wantsNew) startNewTask(null, true)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wantsNew])

  // status columns/groups of the selected project (or of all projects, merged by name)
  const groups = statusGroups(state.statuses, projectFilter)
  const activeProject = state.projects.find((p) => p.id === projectFilter)
  const visibleTasks = filterTasks(state.tasks, {
    currentUserId: state.currentUserId,
    projectId: projectFilter,
    statusKey: statusFilter,
    assigneeId: assigneeFilter,
    statuses: state.statuses,
  })

  const activeTask = taskId ? state.tasks.find((t) => t.id === taskId) : undefined

  return (
    <div className="page tasks-page" data-view={taskId ? 'detail' : 'list'}>
      <ProjectRail projects={state.projects} projectId={projectFilter} onSelect={setProjectFilter} />
      {taskId ? (
        // a task opens as a full page in place of the list (the rail stays)
        <TaskDetail
          key={taskId}
          task={activeTask}
          project={state.projects.find((p) => p.id === activeTask?.projectId)}
          state={state}
          onBack={closeTask}
        />
      ) : (
        <section className="pane tasks-list-pane">
          <div className="pane-header">
            <span className="pane-title">{activeProject?.name ?? 'All tasks'}</span>
            <div className="spacer" />
            <TaskFilters
              users={state.users}
              groups={groups}
              statusKey={statusFilter}
              assigneeId={assigneeFilter}
              sort={sort}
              layout={layout}
              onStatusChange={setStatusFilter}
              onAssigneeChange={setAssigneeFilter}
              onSortChange={setSort}
              onLayoutChange={setLayout}
            />
            <button className="button button-primary" onClick={() => startNewTask()}>
              <Add size={16} />
              New task
            </button>
          </div>
          <div className="pane-body">
            {layout === 'board' ? (
              <TaskBoard
                tasks={visibleTasks}
                users={state.users}
                statuses={state.statuses}
                groups={groups}
                sort={sort}
                activeTaskId={null}
                onOpen={openTask}
              />
            ) : (
              <TaskList
                tasks={visibleTasks}
                users={state.users}
                statuses={state.statuses}
                groups={groups}
                sort={sort}
                onOpen={openTask}
                onAdd={(key) => startNewTask(key)}
              />
            )}
          </div>
        </section>
      )}
    </div>
  )
}
