import { MoreH } from 'reicon-react'
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from '@/components/ui/breadcrumb'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { refIdentifier, type TaskKeyRef } from '@/features/tasks/api/models'
import { breadcrumbParts } from '@/features/tasks/subIssuesLib'

/** Header trail: ancestors (identifier + short title) root first, then the current identifier; >3 ancestors fold the middle. */
export function TaskBreadcrumb({ ancestors, identifier, onOpen }: { ancestors: TaskKeyRef[]; identifier: string; onOpen: (taskId: string) => void }) {
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
        {head.flatMap(crumb)}
        {hidden.length > 0 ? (
          <>
            <BreadcrumbItem>
              <DropdownMenu modal={false}>
                <DropdownMenuTrigger render={
                  <Button variant="ghost" size="icon-xs" className="size-5 text-muted-foreground" aria-label={`Show ${hidden.length} more parents`}>
                    <MoreH />
                  </Button>
                } />
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
