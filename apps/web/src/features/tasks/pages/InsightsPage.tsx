import { Link, useNavigate, useParams } from 'react-router'
import { ArrowLeft, Chart as ChartIcon } from 'reicon-react'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { useCycles } from '@/features/tasks/api/cycles'
import { useProjects } from '@/features/tasks/api/projects'
import { MeasureToggle } from '@/features/tasks/insights/Charts'
import { useMeasure } from '@/features/tasks/insights/chartLib'
import { OpenByChart, ThroughputChart, VelocityChart } from '@/features/tasks/insights/InsightCharts'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** Where the work of a project is, and how much it completes: created and completed, open tasks, velocity. */
export function InsightsPage() {
  const { projectId } = useParams()
  const navigate = useNavigate()
  const { workspace } = useWorkspace()
  const projectsQuery = useProjects(workspace.id)
  const project = projectsQuery.data?.find((item) => item.id === projectId)
  const cycles = useCycles(workspace.id, projectId).data ?? []
  const [measure, setMeasure] = useMeasure(Boolean(project?.estimate_scale))
  const back = `/tasks/projects/${projectId}`
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>
        <PaneHeader>
          <Tip label={project?.name ?? 'Project'} side="bottom">
            <Button variant="ghost" size="icon-sm" className="text-muted-foreground/70" onClick={() => navigate(back)} aria-label="Back to the project">
              <ArrowLeft className="size-4" />
            </Button>
          </Tip>
          {project ? <Link to={back} className="truncate text-[13px] text-muted-foreground outline-none hover:underline focus-visible:underline">{project.name}</Link> : null}
          <span aria-hidden className="text-muted-foreground/50">/</span>
          <PaneTitle render={<h1 />}>Insights</PaneTitle>
          {/* one setting for all charts; a remount applies it to each of them */}
          {project?.estimate_scale ? <div className="ml-auto"><MeasureToggle measure={measure} onChange={setMeasure} /></div> : null}
        </PaneHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {projectsQuery.isPending ? null : !project ? (
            <EmptyState icon={ChartIcon} title="Project not found" description="This project does not exist or was removed." />
          ) : (
            <div key={measure} className="mx-auto flex w-full max-w-[960px] flex-col gap-4 px-10 pt-7 pb-12 max-[899px]:px-5">
              <ThroughputChart project={project} />
              {cycles.length > 0 ? <VelocityChart cycles={cycles} project={project} /> : null}
              <div className="grid grid-cols-2 gap-4 max-[899px]:grid-cols-1">
                <OpenByChart project={project} by="status" />
                <OpenByChart project={project} by="assignee" />
                <OpenByChart project={project} by="priority" />
                <OpenByChart project={project} by="label" />
              </div>
            </div>
          )}
        </div>
      </Pane>
    </div>
  )
}
