import { useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { Add } from 'reicon-react'
import { useAppState } from '../../mock/store'
import type { TaskStatus } from '../../mock/types'
import { NewTaskModal } from './components/NewTaskModal'
import { ProjectRail } from './components/ProjectRail'
import { TaskBoard } from './components/TaskBoard'
import { TaskDetail } from './components/TaskDetail'
import { TaskFilters } from './components/TaskFilters'
import { TaskList } from './components/TaskList'
import { filterTasks } from './tasksLib'
import './tasks.css'

export function TasksPage() {
  const { taskId } = useParams()
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  const state = useAppState()

  const [tab, setTab] = useState<'my' | 'all'>('my')
  const [layout, setLayout] = useState<'list' | 'board'>('list')
  const [statusFilter, setStatusFilter] = useState<TaskStatus | null>(null)
  const [assigneeFilter, setAssigneeFilter] = useState<string | null>(null)
  // "New task" from the header or from a status group's "+" (which presets the status)
  const [newTask, setNewTask] = useState<{ status: TaskStatus } | null>(null)

  // the topbar "New → Task" entry deep-links here with ?new=1
  const showNewTask = newTask !== null || searchParams.get('new') === '1'
  const closeNewTask = () => {
    setNewTask(null)
    if (searchParams.get('new')) {
      const next = new URLSearchParams(searchParams)
      next.delete('new')
      setSearchParams(next, { replace: true })
    }
  }

  const projectFilter = searchParams.get('project')
  const persisted = new URLSearchParams(searchParams)
  persisted.delete('new')
  const search = persisted.toString()
  const searchSuffix = search ? `?${search}` : ''

  const setProjectFilter = (projectId: string | null) => {
    const next = new URLSearchParams(searchParams)
    if (projectId) next.set('project', projectId)
    else next.delete('project')
    setSearchParams(next, { replace: true })
  }

  const openTask = (id: string) => navigate(`/tasks/${id}${searchSuffix}`)
  const closeTask = () => navigate(`/tasks${searchSuffix}`)

  const visibleTasks = filterTasks(state.tasks, {
    tab,
    currentUserId: state.currentUserId,
    projectId: projectFilter,
    status: statusFilter,
    assigneeId: assigneeFilter,
  })

  const activeTask = taskId ? state.tasks.find((t) => t.id === taskId) : undefined

  return (
    <div className="page tasks-page" data-view={taskId ? 'detail' : 'list'}>
      <ProjectRail projects={state.projects} tasks={state.tasks} projectId={projectFilter} onSelect={setProjectFilter} />
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
            <button className="app-tab" data-active={tab === 'my' || undefined} onClick={() => setTab('my')}>
              My tasks
            </button>
            <button className="app-tab" data-active={tab === 'all' || undefined} onClick={() => setTab('all')}>
              All
            </button>
            <div className="spacer" />
            <TaskFilters
              users={state.users}
              status={statusFilter}
              assigneeId={assigneeFilter}
              layout={layout}
              onStatusChange={setStatusFilter}
              onAssigneeChange={setAssigneeFilter}
              onLayoutChange={setLayout}
            />
            <button className="button button-primary" onClick={() => setNewTask({ status: 'todo' })}>
              <Add size={16} />
              New task
            </button>
          </div>
          <div className="pane-body">
            {layout === 'board' ? (
              <TaskBoard tasks={visibleTasks} users={state.users} activeTaskId={null} onOpen={openTask} />
            ) : (
              <TaskList tasks={visibleTasks} users={state.users} onOpen={openTask} onAdd={(status) => setNewTask({ status })} />
            )}
          </div>
        </section>
      )}

      {showNewTask ? (
        <NewTaskModal
          projects={state.projects}
          defaultProjectId={projectFilter}
          defaultStatus={newTask?.status ?? 'todo'}
          onClose={closeNewTask}
          onCreated={(id) => {
            closeNewTask()
            openTask(id)
          }}
        />
      ) : null}
    </div>
  )
}
