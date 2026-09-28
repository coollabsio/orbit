import { MoreH } from 'reicon-react'
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from '@/components/ui/breadcrumb'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { refIdentifier, type TaskKeyRef } from '@/features/tasks/api/models'
import { breadcrumbParts } from '@/features/tasks/subIssuesLib'

const CRUMB =
  'flex min-w-0 items-center gap-1.5 rounded-sm text-xs text-muted-foreground/70 outline-none transition-colors duration-150 hover-fine:hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 motion-reduce:transition-none'
const SEPARATOR = 'text-muted-foreground/50 [&>svg]:size-3'

/** Header trail: ancestors (identifier + short title) root first, then the current identifier; >3 ancestors fold the middle. */
export function TaskBreadcrumb({ ancestors, identifier, onOpen }: { ancestors: TaskKeyRef[]; identifier: string; onOpen: (taskId: string) => void }) {
  const { head, hidden, tail } = breadcrumbParts(ancestors)
  const crumb = (ref: TaskKeyRef) => [
    <BreadcrumbItem key={ref.id} className="min-w-0">
      <BreadcrumbLink render={<button type="button" onClick={() => onOpen(ref.id)} />} className={CRUMB}>
        <span className="shrink-0 tabular-nums">{refIdentifier(ref)}</span>
        {/* titles give way first: identifiers alone on narrow screens */}
        <span className="max-w-[160px] truncate max-[899px]:hidden">{ref.title || 'Untitled'}</span>
      </BreadcrumbLink>
    </BreadcrumbItem>,
    <BreadcrumbSeparator key={`${ref.id}:separator`} className={SEPARATOR} />,
  ]
  return (
    <Breadcrumb className="min-w-0">
      <BreadcrumbList className="flex-nowrap gap-1.5 text-xs">
        {head.flatMap(crumb)}
        {hidden.length > 0 ? (
          <>
            <BreadcrumbItem>
              <DropdownMenu modal={false}>
                <DropdownMenuTrigger render={
                  <Button variant="ghost" size="icon-xs" className="size-5 text-muted-foreground/70" aria-label={`Show ${hidden.length} more parents`}>
                    <MoreH className="size-3.5" />
                  </Button>
                } />
                <DropdownMenuContent align="start" className="flex w-auto max-w-[320px] min-w-[220px] flex-col gap-px p-1">
                  {hidden.map((ref) => (
                    <DropdownMenuItem key={ref.id} className="min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm" onClick={() => onOpen(ref.id)}>
                      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{refIdentifier(ref)}</span>
                      <span className="truncate">{ref.title || 'Untitled'}</span>
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </BreadcrumbItem>
            <BreadcrumbSeparator className={SEPARATOR} />
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
