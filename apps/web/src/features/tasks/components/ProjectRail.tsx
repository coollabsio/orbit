import { useNavigate } from 'react-router'
import { Add, Setting2, TaskSquare } from 'reicon-react'
import { useState } from 'react'
import type { Project } from '../api/models'
import { NewProjectModal } from './NewProjectModal'

interface ProjectRailProps {
  projects: Project[]
  projectId: string | null
  onSelect: (projectId: string | null) => void
}

/** Second sidebar for Tasks: "All projects" + one row per project (color square, name, hover gear → settings). */
export function ProjectRail({ projects, projectId, onSelect }: ProjectRailProps) {
  const navigate = useNavigate()
  const [showNewProject, setShowNewProject] = useState(false)
  return (
    <section className="pane tasks-rail-pane">
      <div className="pane-header">
        <span className="pane-title">Projects</span>
      </div>
      <div className="pane-body tasks-rail-body">
        <button className="menu-item" data-active={projectId === null || undefined} onClick={() => onSelect(null)}>
          <TaskSquare size={16} />
          <span className="menu-item-label">All projects</span>
        </button>
        {projects.map((project) => (
          <div key={project.id} className="tasks-rail-row">
            <button
              className="menu-item"
              data-active={project.id === projectId || undefined}
              onClick={() => onSelect(project.id)}
            >
              <span className="pill-dot tasks-rail-dot" style={{ background: project.color }} />
              <span className="menu-item-label">{project.name}</span>
            </button>
            <button
              type="button"
              className="icon-button tasks-rail-settings"
              aria-label={`${project.name} settings`}
              title="Project settings"
              onClick={() => navigate(`/tasks/projects/${project.id}/settings`)}
            >
              <Setting2 size={14} />
            </button>
          </div>
        ))}
        <button className="menu-item" onClick={() => setShowNewProject(true)}><Add size={16} /><span className="menu-item-label">New project</span></button>
      </div>
      {showNewProject ? <NewProjectModal onClose={() => setShowNewProject(false)} onCreated={(project) => { onSelect(project.id); setShowNewProject(false) }} /> : null}
    </section>
  )
}
