import { useNavigate } from 'react-router'
import { Plus, Settings, SquareCheck } from 'lucide-react'
import { useState } from 'react'
import type { Project } from '../api/models'
import { NewProjectModal } from './NewProjectModal'

const MENU_ITEM =
  'relative flex h-8 w-full min-w-0 items-center gap-2.5 overflow-hidden rounded-md px-2.5 text-[13px] font-medium whitespace-nowrap text-sidebar-foreground transition-colors hover:bg-sidebar-accent/50 hover:text-sidebar-accent-foreground data-[active]:bg-sidebar-accent data-[active]:text-sidebar-accent-foreground'

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
    <section className="flex h-full min-h-0 w-60 shrink-0 flex-col bg-background">
      <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border px-3 py-2">
        <span className="truncate text-[13px] font-semibold text-foreground">Projects</span>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto p-2">
        <button className={MENU_ITEM} data-active={projectId === null || undefined} onClick={() => onSelect(null)}>
          <SquareCheck className="size-4.5 shrink-0 opacity-90" />
          <span className="min-w-0 flex-1 truncate">All projects</span>
        </button>
        {projects.map((project) => (
          <div key={project.id} className="group/row relative flex items-center">
            <button
              className={`${MENU_ITEM} flex-1 pr-8`}
              data-active={project.id === projectId || undefined}
              onClick={() => onSelect(project.id)}
            >
              <span className="mx-[5px] size-2 shrink-0 rounded-[2px]" style={{ background: project.color }} />
              <span className="min-w-0 flex-1 truncate">{project.name}</span>
            </button>
            <button
              type="button"
              className="absolute right-1.5 inline-flex size-6 items-center justify-center rounded-md text-muted-foreground/70 opacity-0 transition hover:bg-accent hover:text-foreground group-hover/row:opacity-100 focus-visible:opacity-100"
              aria-label={`${project.name} settings`}
              title="Project settings"
              onClick={() => navigate(`/tasks/projects/${project.id}/settings`)}
            >
              <Settings className="size-3.5" />
            </button>
          </div>
        ))}
        <button className={MENU_ITEM} onClick={() => setShowNewProject(true)}><Plus className="size-4.5 shrink-0 opacity-90" /><span className="min-w-0 flex-1 truncate">New project</span></button>
      </div>
      {showNewProject ? <NewProjectModal onClose={() => setShowNewProject(false)} onCreated={(project) => { onSelect(project.id); setShowNewProject(false) }} /> : null}
    </section>
  )
}
