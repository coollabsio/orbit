import { useNavigate } from 'react-router'
import { Add as Plus, Setting2 as Settings, TaskSquare as SquareCheck } from 'reicon-react'
import { useState, type ComponentProps } from 'react'
import { cn } from 'cn'
import type { Project } from '@/features/tasks/api/models'
import { Button } from '@/components/ui/button'
import { PaneHeader, PaneTitle } from '@/components/common/Pane'
import { NewProjectModal } from './NewProjectModal'

interface ProjectRailProps {
  projects: Project[]
  projectId: string | null
  onSelect: (projectId: string | null) => void
}

/** Second sidebar for Tasks: "All projects" + one row per project (color square, name, settings gear). */
export function ProjectRail({ projects, projectId, onSelect }: ProjectRailProps) {
  const navigate = useNavigate()
  const [showNewProject, setShowNewProject] = useState(false)
  return (
    <section className="flex h-full min-h-0 w-60 shrink-0 flex-col bg-background">
      {/* the rail keeps its bottom border on phones */}
      <PaneHeader className="max-[899px]:border-b">
        <PaneTitle>Projects</PaneTitle>
      </PaneHeader>
      <div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto p-2">
        <RailItem active={projectId === null} onClick={() => onSelect(null)}>
          <SquareCheck className="size-4.5 opacity-90" />
          <span className="min-w-0 flex-1 truncate">All projects</span>
        </RailItem>
        {projects.map((project) => (
          <div key={project.id} className="relative flex items-center">
            <RailItem className="flex-1 pr-8" active={project.id === projectId} onClick={() => onSelect(project.id)}>
              <span className="mx-[5px] size-2 shrink-0 rounded-[2px]" style={{ background: project.color }} />
              <span className="min-w-0 flex-1 truncate">{project.name}</span>
            </RailItem>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="absolute right-1.5 text-muted-foreground"
              aria-label={`${project.name} settings`}
              title="Project settings"
              onClick={() => navigate(`/tasks/projects/${project.id}/settings`)}
            >
              <Settings className="size-3.5" />
            </Button>
          </div>
        ))}
        <RailItem onClick={() => setShowNewProject(true)}>
          <Plus className="size-4.5 opacity-90" />
          <span className="min-w-0 flex-1 truncate">New project</span>
        </RailItem>
      </div>
      {showNewProject ? <NewProjectModal onClose={() => setShowNewProject(false)} onCreated={(project) => { onSelect(project.id); setShowNewProject(false) }} /> : null}
    </section>
  )
}

/** A rail row: full-width ghost button; the current project keeps the sidebar's active fill. */
function RailItem({ active, className, ...props }: ComponentProps<typeof Button> & { active?: boolean }) {
  return (
    <Button
      variant="ghost"
      data-active={active || undefined}
      className={cn('w-full min-w-0 shrink justify-start gap-2.5 text-[13px] data-active:bg-sidebar-accent data-active:text-sidebar-accent-foreground', className)}
      {...props}
    />
  )
}
