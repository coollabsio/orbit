import { TaskSquare } from 'reicon-react'
import type { Project, Task } from '../../../mock/types'

interface ProjectRailProps {
  projects: Project[]
  tasks: Task[]
  projectId: string | null
  onSelect: (projectId: string | null) => void
}

const openCount = (tasks: Task[]) => tasks.filter((t) => t.status === 'todo' || t.status === 'in_progress').length

/** Second sidebar for Tasks (same shape as the Docs tree): all projects + one row per project. */
export function ProjectRail({ projects, tasks, projectId, onSelect }: ProjectRailProps) {
  return (
    <section className="pane tasks-rail-pane">
      <div className="pane-header">
        <span className="pane-title">Projects</span>
      </div>
      <div className="pane-body tasks-rail-body">
        <button className="menu-item" data-active={projectId === null || undefined} onClick={() => onSelect(null)}>
          <TaskSquare size={16} />
          <span className="menu-item-label">All projects</span>
          <span className="tasks-rail-count">{openCount(tasks)}</span>
        </button>
        <div className="nav-section" style={{ marginTop: 12 }}>
          Projects
        </div>
        {projects.map((project) => (
          <button
            key={project.id}
            className="menu-item"
            data-active={project.id === projectId || undefined}
            onClick={() => onSelect(project.id)}
          >
            <span className="pill-dot tasks-rail-dot" style={{ background: project.color }} />
            <span className="menu-item-label">{project.name}</span>
            <span className="tasks-rail-key">{project.key}</span>
            <span className="tasks-rail-count">{openCount(tasks.filter((t) => t.projectId === project.id))}</span>
          </button>
        ))}
      </div>
    </section>
  )
}
