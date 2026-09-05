import { useNavigate } from 'react-router'
import { Add, Setting2, TaskSquare } from 'reicon-react'
import type { ProjectBody } from '../../../api/generated/types.gen'
import type { Project } from '../api/models'
import { useCreateProject } from '../api/projects'
import { useWorkspace } from '../../workspaces/workspaceContext'
import { projectDraft } from '../api/projectDraft'

interface ProjectRailProps {
  projects: Project[]
  projectId: string | null
  onSelect: (projectId: string | null) => void
}

/** Second sidebar for Tasks: "All projects" + one row per project (color square, name, hover gear → settings). */
export function ProjectRail({ projects, projectId, onSelect }: ProjectRailProps) {
  const navigate = useNavigate()
  const { workspace } = useWorkspace()
  const createProject = useCreateProject(workspace.id)
  const createAndSelectProject = (body: ProjectBody) => createProject.mutate(body, {
    onSuccess: (project) => onSelect(project.id),
  })
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
        <button className="menu-item" disabled={createProject.isPending} onClick={() => {
          const name = window.prompt('Project name')?.trim()
          if (!name) return
          createAndSelectProject(projectDraft(name))
        }}><Add size={16} /><span className="menu-item-label">New project</span></button>
        {createProject.isError ? <p role="alert" className="text-danger text-xs">Project creation failed. <button className="button button-ghost" onClick={() => createProject.variables && createAndSelectProject(createProject.variables)}>Retry</button></p> : null}
      </div>
    </section>
  )
}
