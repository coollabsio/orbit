import { MoreH } from 'reicon-react'
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from '@/components/ui/breadcrumb'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { ColorDot } from '@/components/common/ColorDot'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { refIdentifier, type Project, type TaskKeyRef } from '@/features/tasks/api/models'
import { breadcrumbParts } from '@/features/tasks/subIssuesLib'

interface TaskBreadcrumbProps {
  /** The task's project: the first crumb, which opens the project's task list. */
  project?: Project
  ancestors: TaskKeyRef[]
  identifier: string
  onOpen: (taskId: string) => void
  onOpenProject?: (projectId: string) => void
}

/** Header trail: the project, ancestors (identifier + short title) root first, then the current identifier; >3 ancestors fold the middle. */
export function TaskBreadcrumb({ project, ancestors, identifier, onOpen, onOpenProject }: TaskBreadcrumbProps) {
  const { head, hidden, tail } = breadcrumbParts(ancestors)
  const crumb = (ref: TaskKeyRef) => [
    <BreadcrumbItem key={ref.id} className="min-w-0">
      <BreadcrumbLink
        render={<Button type="button" variant="link" onClick={() => onOpen(ref.id)} />}
        className="h-auto min-w-0 p-0 text-xs font-normal text-muted-foreground"
      >
        <span className="shrink-0 tabular-nums">{refIdentifier(ref)}</span>
        {/* titles give way first: identifiers alone on narrow screens */}
        <span className="max-w-40 truncate max-[899px]:hidden">{ref.title || 'Untitled'}</span>
      </BreadcrumbLink>
    </BreadcrumbItem>,
    <BreadcrumbSeparator key={`${ref.id}:separator`} />,
  ]
  return (
    <Breadcrumb className="min-w-0">
      <BreadcrumbList className="flex-nowrap text-xs">
        {project ? (
          <>
            <BreadcrumbItem className="min-w-0">
              <Tip label={`Open ${project.name}`} side="bottom">
                <BreadcrumbLink
                  render={<Button type="button" variant="link" onClick={() => onOpenProject?.(project.id)} />}
                  className="h-auto min-w-0 gap-1.5 p-0 text-xs font-medium text-foreground"
                >
                  <ColorDot color={project.color} />
                  <span className="max-w-48 truncate max-[899px]:max-w-28">{project.name}</span>
                </BreadcrumbLink>
              </Tip>
            </BreadcrumbItem>
            <BreadcrumbSeparator />
          </>
        ) : null}
        {head.flatMap(crumb)}
        {hidden.length > 0 ? (
          <>
            <BreadcrumbItem>
              <DropdownMenu modal={false}>
                <Tip label={`Show ${hidden.length} more parents`} side="bottom">
                  <DropdownMenuTrigger render={
                    <Button variant="ghost" size="icon-xs" className="size-5 text-muted-foreground" aria-label={`Show ${hidden.length} more parents`}>
                      <MoreH />
                    </Button>
                  } />
                </Tip>
                <DropdownMenuContent align="start" className="w-auto max-w-80 min-w-55">
                  {hidden.map((ref) => (
                    <DropdownMenuItem key={ref.id} onClick={() => onOpen(ref.id)}>
                      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{refIdentifier(ref)}</span>
                      <span className="truncate">{ref.title || 'Untitled'}</span>
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </BreadcrumbItem>
            <BreadcrumbSeparator />
          </>
        ) : null}
        {tail.flatMap(crumb)}
        <BreadcrumbItem>
          <BreadcrumbPage className="text-xs text-muted-foreground tabular-nums">{identifier}</BreadcrumbPage>
        </BreadcrumbItem>
      </BreadcrumbList>
    </Breadcrumb>
  )
}
