import { Calendar, Sort as ArrowUpDown, Kanban as Columns3, List, SearchNormal as Search, Setting4 as SlidersHorizontal } from 'reicon-react'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { SORT_OPTIONS, type SortKey, type TaskLayout } from '@/features/tasks/tasksLib'

const MENU = 'flex w-auto min-w-[180px] flex-col gap-px p-1'
const OPTION =
  `group min-h-8 cursor-pointer gap-2 px-2 py-1.5 text-sm font-normal whitespace-normal text-foreground [&_svg:not([class*='size-'])]:size-3.5 data-[selected]:bg-accent data-[selected]:font-medium`
const HEADING = 'px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase'
const FILTER_BTN =
  'data-[active]:bg-primary/10 data-[active]:text-primary data-[active]:ring-1 data-[active]:ring-inset data-[active]:ring-primary/25 max-[899px]:w-8 max-[899px]:px-0'
/** Shared control tokens on the wrapper; the single focus ring lives there too, so the inner input adds none. */
const SEARCH_GROUP =
  'h-8 w-auto min-w-[180px] rounded-lg border border-input bg-muted transition-colors has-[[data-slot=input-group-control]:focus-visible]:border-ring has-[[data-slot=input-group-control]:focus-visible]:ring-1 has-[[data-slot=input-group-control]:focus-visible]:ring-ring dark:bg-muted max-[899px]:order-10 max-[899px]:mt-1 max-[899px]:h-[34px] max-[899px]:min-w-0 max-[899px]:basis-full'

interface TaskFiltersProps {
  sort: SortKey
  layout: TaskLayout
  search: string
  onSortChange: (sort: SortKey) => void
  onLayoutChange: (layout: TaskLayout) => void
  onSearchChange: (search: string) => void
}

/** Interim header controls: quick search, "Sort" and "Display" (filters moved to FilterButton / FilterBar). */
export function TaskFilters({ sort, layout, onSortChange, onLayoutChange, search, onSearchChange }: TaskFiltersProps) {
  const sortLabel = SORT_OPTIONS.find((o) => o.key === sort)?.label ?? 'Sort'

  return (
    <>
      <InputGroup className={SEARCH_GROUP}>
        <InputGroupAddon align="inline-start" className="pl-[9px]">
          <Search className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        </InputGroupAddon>
        <InputGroupInput type="search" aria-label="Search tasks" placeholder="Search tasks" value={search} onChange={(event) => onSearchChange(event.target.value)} className="h-auto border-0 text-sm text-foreground shadow-none outline-none placeholder:text-muted-foreground focus:outline-none focus-visible:ring-0 md:text-sm" />
      </InputGroup>

      {/* Timeline rows always sort by date, so manual/priority sort does not apply. */}
      {layout === 'timeline' ? null : <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button variant="ghost" className={FILTER_BTN} data-active={sort !== 'manual' || undefined} aria-label={`Sort tasks: ${sortLabel}`}>
              <ArrowUpDown className="size-4" />
              <span className="max-[899px]:hidden">{sort === 'manual' ? 'Sort' : sortLabel}</span>
            </Button>
          }
        />
        <DropdownMenuContent align="end" className={MENU}>
          <DropdownMenuGroup className="flex flex-col gap-px">
            <DropdownMenuLabel className={HEADING}>Sort by</DropdownMenuLabel>
            {SORT_OPTIONS.map((option) => (
              <DropdownMenuItem
                key={option.key}
                className={OPTION}
                data-selected={option.key === sort || undefined}
                onClick={() => onSortChange(option.key)}
              >
                {option.label}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>}

      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button variant="ghost" className={FILTER_BTN} aria-label="Display options">
              <SlidersHorizontal className="size-4" />
              <span className="max-[899px]:hidden">Display</span>
            </Button>
          }
        />
        <DropdownMenuContent align="end" className={MENU}>
          <DropdownMenuGroup className="flex flex-col gap-px">
            <DropdownMenuLabel className={HEADING}>Layout</DropdownMenuLabel>
            <DropdownMenuItem className={OPTION} data-selected={layout === 'list' || undefined} onClick={() => onLayoutChange('list')}>
              <List className="size-3.5" />
              List
            </DropdownMenuItem>
            <DropdownMenuItem className={OPTION} data-selected={layout === 'board' || undefined} onClick={() => onLayoutChange('board')}>
              <Columns3 className="size-3.5" />
              Board
            </DropdownMenuItem>
            <DropdownMenuItem className={OPTION} data-selected={layout === 'timeline' || undefined} onClick={() => onLayoutChange('timeline')}>
              <Calendar className="size-3.5" />
              Timeline
            </DropdownMenuItem>
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  )
}
