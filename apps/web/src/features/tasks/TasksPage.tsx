import { useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { Add } from 'reicon-react'
import { useAppState } from '../../mock/store'
import type { TaskStatus } from '../../mock/types'
import { NewTaskModal } from './components/NewTaskModal'
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
  const [statusFilter, setStatusFilter] = useState<TaskStatus | null>(null)
  const [assigneeFilter, setAssigneeFilter] = useState<string | null>(null)
  const [showNewTask, setShowNewTask] = useState(false)

  const projectFilter = searchParams.get('project')
  const search = searchParams.toString()
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
      <section className="pane tasks-list-pane">
        <div className="pane-header">
          <span className="pane-title">Tasks</span>
          <button className="app-tab" data-active={tab === 'my' || undefined} onClick={() => setTab('my')}>
            My tasks
          </button>
          <button className="app-tab" data-active={tab === 'all' || undefined} onClick={() => setTab('all')}>
            All
          </button>
          <div className="spacer" />
          <button className="button button-primary" onClick={() => setShowNewTask(true)}>
            <Add size={16} />
            New task
          </button>
        </div>
        <TaskFilters
          projects={state.projects}
          users={state.users}
          projectId={projectFilter}
          status={statusFilter}
          assigneeId={assigneeFilter}
          onProjectChange={setProjectFilter}
          onStatusChange={setStatusFilter}
          onAssigneeChange={setAssigneeFilter}
        />
        <div className="pane-body">
          <TaskList
            tasks={visibleTasks}
            users={state.users}
            activeTaskId={taskId ?? null}
            onOpen={openTask}
          />
        </div>
      </section>

      {taskId ? (
        <TaskDetail
          key={taskId}
          task={activeTask}
          project={state.projects.find((p) => p.id === activeTask?.projectId)}
          users={state.users}
          onBack={closeTask}
        />
      ) : null}

      {showNewTask ? (
        <NewTaskModal
          projects={state.projects}
          defaultProjectId={projectFilter}
          onClose={() => setShowNewTask(false)}
          onCreated={(id) => {
            setShowNewTask(false)
            openTask(id)
          }}
        />
      ) : null}
    </div>
  )
}
