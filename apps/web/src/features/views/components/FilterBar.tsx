import { useCommand, useShortcutLabel } from '@/shortcuts/useCommand'
import { useEffect, useState, type ComponentProps, type ReactElement, type ReactNode } from 'react'
import { Add as Plus, Filter, Hierarchy, Lock, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from '@/components/ui/command'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  FIELD_META,
  FIELD_ORDER,
  MAX_FILTER_CONDITIONS,
  defaultValue,
  isCompleteCondition,
  listValue,
  operatorLabel,
  operatorNeedsValue,
  selectedOptions,
  valueSummary,
  withOperator,
  type FilterOptions,
} from '../filterFields'
import { appendCondition } from '../filterTree'
import { countConditions, isFlatFilter, type Condition, type FilterField, type FilterGroup, type FilterOperator } from '../viewState'
import { FilterValuePicker, ValueGlyph } from './FilterValuePicker'

/** Title of the add controls once the tree is at the cap. */
const FULL_TITLE = `Filters can have at most ${MAX_FILTER_CONDITIONS} conditions`

interface AddFilterPopoverProps {
  filter: FilterGroup
  options: FilterOptions
  onChange: (filter: FilterGroup) => void
  onOpenAdvanced?: () => void
  open: boolean
  onOpenChange: (open: boolean) => void
  instant: boolean
  trigger: ReactElement
}

/** Field list → value picker in one popover. A new condition joins the tree once it is complete. */
function AddFilterPopover({ filter, options, onChange, onOpenAdvanced, open, onOpenChange, instant, trigger }: AddFilterPopoverProps) {
  const [draft, setDraft] = useState<Condition | null>(null)
  // the tree before the draft joined it; undoing the draft restores it exactly (appendCondition may wrap the root)
  const [base, setBase] = useState<FilterGroup | null>(null)
  // each open starts at the field list; the draft stays while closing, so the exit animation keeps the value picker
  const [wasOpen, setWasOpen] = useState(open)
  if (open !== wasOpen) {
    setWasOpen(open)
    if (open) {
      setDraft(null)
      setBase(null)
    }
  }
  const close = () => onOpenChange(false)
  const pickField = (field: FilterField) => {
    const operator = FIELD_META[field].defaultOperator
    setDraft({ field, operator, value: defaultValue(field, operator) })
  }
  const change = (next: Condition) => {
    setDraft(next)
    if (isCompleteCondition(next)) {
      onChange(appendCondition(base ?? filter, next))
      setBase(base ?? filter)
    } else if (base) {
      onChange(base)
      setBase(null)
    }
  }
  return (
    <Popover open={open} onOpenChange={(next) => (next ? onOpenChange(true) : close())} modal={false}>
      <PopoverTrigger render={trigger} />
      <PopoverContent align="start" {...(instant && { 'data-instant': '' })} className="w-auto gap-0 p-0 data-instant:animate-none">
        {draft ? (
          <FilterValuePicker condition={draft} options={options} onChange={change} onDone={close} />
        ) : (
          <Command label="Filter by" className="w-60">
            <CommandInput autoFocus aria-label="Filter by" placeholder="Filter by…" />
            <CommandList>
              <CommandEmpty className="py-4 text-xs text-muted-foreground">No matching fields</CommandEmpty>
              <CommandGroup>
                {FIELD_ORDER.map((field) => {
                  const Icon = FIELD_META[field].icon
                  return (
                    <CommandItem key={field} value={field} keywords={[FIELD_META[field].label]} onSelect={() => pickField(field)}>
                      <Icon className="size-3.5 text-muted-foreground" aria-hidden="true" />
                      {FIELD_META[field].label}
                    </CommandItem>
                  )
                })}
              </CommandGroup>
              {onOpenAdvanced ? (
                <>
                  <CommandSeparator />
                  <CommandGroup>
                    <CommandItem
                      value="advanced"
                      keywords={['Advanced filter']}
                      onSelect={() => {
                        close()
                        onOpenAdvanced()
                      }}
                    >
                      <Hierarchy className="size-3.5 text-muted-foreground" aria-hidden="true" />
                      Advanced filter
                    </CommandItem>
                  </CommandGroup>
                </>
              ) : null}
            </CommandList>
          </Command>
        )}
      </PopoverContent>
    </Popover>
  )
}

export interface FilterButtonProps {
  filter: FilterGroup
  options: FilterOptions
  onChange: (filter: FilterGroup) => void
  onOpenAdvanced?: () => void
}

/** Header "Filter" button. `F` opens it (never while typing); keyboard opens skip the animation. */
export function FilterButton({ filter, options, onChange, onOpenAdvanced }: FilterButtonProps) {
  const [open, setOpen] = useState(false)
  const [instant, setInstant] = useState(false)
  const count = countConditions(filter)
  // at the limit a new condition could push the effective tree past the server's 50
  const full = count >= MAX_FILTER_CONDITIONS
  useCommand('view.filter', () => {
    setInstant(true)
    setOpen(true)
  }, { enabled: !full })
  const keyShortcuts = useShortcutLabel('view.filter')
  return (
    <AddFilterPopover
      filter={filter}
      options={options}
      onChange={onChange}
      onOpenAdvanced={onOpenAdvanced}
      open={open}
      instant={instant}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) setInstant(false)
      }}
      trigger={
        <Button type="button" variant="ghost" aria-label="Filter tasks" aria-keyshortcuts={keyShortcuts} disabled={full} title={full ? FULL_TITLE : undefined} data-active={count > 0 || undefined} className="data-active:bg-primary/10 data-active:text-primary max-[899px]:w-8 max-[899px]:px-0">
          <Filter className="size-4" />
          <span className="max-[899px]:hidden">Filter</span>
          {count > 0 ? (
            <Badge className="h-4 min-w-4 px-1 text-[10px] tabular-nums">{count}</Badge>
          ) : null}
        </Button>
      }
    />
  )
}

export interface FilterBarProps {
  filter: FilterGroup
  options: FilterOptions
  onChange: (filter: FilterGroup) => void
  /** Non-removable chip for the page's preset scope, e.g. "Overdue". */
  presetLabel?: string | null
  onOpenAdvanced?: () => void
  /** Right-aligned controls for the row (e.g. Save view); they show the row even without a filter. */
  actions?: ReactNode
}

/** The chip row under the header. Renders nothing when there is no scope, no filter, and no action. */
export function FilterBar({ filter, options, onChange, presetLabel = null, onOpenAdvanced, actions }: FilterBarProps) {
  const [live, setLive] = useState(false)
  const [adding, setAdding] = useState(false)
  // the same cap as FilterButton: a 47th condition could push a scoped page past the server's 50
  const full = countConditions(filter) >= MAX_FILTER_CONDITIONS
  // chips present on first paint appear as-is; only chips added later animate in
  useEffect(() => {
    const frame = requestAnimationFrame(() => setLive(true))
    return () => cancelAnimationFrame(frame)
  }, [])
  if (!presetLabel && filter.children.length === 0 && !actions) return null
  const flat = isFlatFilter(filter)
  const replaceAt = (index: number, next: Condition) => onChange({ ...filter, children: filter.children.map((child, i) => (i === index ? next : child)) })
  const removeAt = (index: number) => onChange({ ...filter, children: filter.children.filter((_, i) => i !== index) })
  return (
    <div role="toolbar" aria-label="Filters" className="flex min-h-10 shrink-0 flex-wrap items-center gap-1.5 border-b bg-background px-3 py-1.5 max-[899px]:px-2">
      {presetLabel ? (
        <span
          title="Always applied on this page"
          className="inline-flex h-7 items-center gap-1.5 rounded-md border border-primary/25 bg-primary/10 px-2 text-xs font-medium text-primary"
        >
          <Lock className="size-3" aria-hidden="true" />
          {presetLabel}
        </span>
      ) : null}
      {flat ? (
        filter.children.map((node, index) => (
          <ConditionChip
            key={index}
            condition={node as Condition}
            options={options}
            enter={live}
            onChange={(next) => replaceAt(index, next)}
            onRemove={() => removeAt(index)}
          />
        ))
      ) : (
        <AdvancedChip count={countConditions(filter)} enter={live} onOpen={onOpenAdvanced} />
      )}
      <AddFilterPopover
        filter={filter}
        options={options}
        onChange={onChange}
        onOpenAdvanced={onOpenAdvanced}
        open={adding}
        instant={false}
        onOpenChange={setAdding}
        trigger={
          <Button type="button" variant="ghost" size="icon-xs" aria-label="Add filter" disabled={full} title={full ? FULL_TITLE : undefined} className="text-muted-foreground">
            <Plus />
          </Button>
        }
      />
      {actions ? <div className="ml-auto flex shrink-0 items-center gap-1.5">{actions}</div> : null}
    </div>
  )
}

interface ConditionChipProps {
  condition: Condition
  options: FilterOptions
  enter: boolean
  onChange: (next: Condition) => void
  onRemove: () => void
}

/** `Field · operator · values ×` as one segmented pill. */
function ConditionChip({ condition, options, enter, onChange, onRemove }: ConditionChipProps) {
  const [animate] = useState(enter)
  const [draft, setDraft] = useState<Condition | null>(null)
  const [operatorOpen, setOperatorOpen] = useState(false)
  const meta = FIELD_META[condition.field]
  const Icon = meta.icon
  const shown = draft ?? condition
  const count = meta.kind === 'list' || meta.kind === 'task' ? listValue(shown).length : 1
  const glyphs = selectedOptions(shown, options).slice(0, 3)
  const summary = valueSummary(shown, options)
  const operatorText = operatorLabel(shown.operator, count)

  const edit = (next: Condition) => {
    setDraft(next)
    if (isCompleteCondition(next)) onChange(next)
  }
  // emptying the values of a chip removes it; abandoning an operator switch keeps the old condition
  const closeValues = () => {
    if (draft && !isCompleteCondition(draft) && draft.operator === condition.operator) onRemove()
    setDraft(null)
  }
  const pickOperator = (operator: FilterOperator) => {
    setOperatorOpen(false)
    const next = withOperator(condition, operator)
    if (isCompleteCondition(next)) onChange(next)
    else setDraft(next)
  }

  return (
    <FilterChip data-filter-chip={condition.field} className={cn(animate && 'animate-filter-chip-enter')}>
      <ChipSegment>
        <Icon className="size-3.5" aria-hidden="true" />
        {meta.label}
      </ChipSegment>
      {meta.operators.length > 1 ? (
        <DropdownMenu open={operatorOpen} onOpenChange={setOperatorOpen} modal={false}>
          <DropdownMenuTrigger
            render={
              <ChipButton className="text-muted-foreground" aria-label={`${meta.label} operator: ${operatorText}`}>
                {operatorText}
              </ChipButton>
            }
          />
          {operatorOpen ? (
            <DropdownMenuContent align="start" className="w-auto min-w-40">
              {meta.operators.map((operator) => (
                <DropdownMenuItem key={operator} data-selected={operator === shown.operator || undefined} className="data-selected:font-medium" onClick={() => pickOperator(operator)}>
                  {operatorLabel(operator, count)}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          ) : null}
        </DropdownMenu>
      ) : (
        <ChipSegment>{operatorText}</ChipSegment>
      )}
      {operatorNeedsValue(shown.operator) ? (
        <Popover open={draft !== null} onOpenChange={(open) => (open ? setDraft(condition) : closeValues())} modal={false}>
          <PopoverTrigger
            render={
              <ChipButton className="font-medium" aria-label={`${meta.label} values: ${summary || 'none'}`}>
                {glyphs.length > 0 ? (
                  <span aria-hidden="true" className="flex items-center gap-0.5">
                    {glyphs.map((option) => <ValueGlyph key={option.value} glyph={option.glyph} size={12} />)}
                  </span>
                ) : null}
                <span className="max-w-56 truncate">{summary || 'Choose…'}</span>
              </ChipButton>
            }
          />
          {draft ? (
            <PopoverContent align="start" className="w-auto gap-0 p-0">
              <FilterValuePicker condition={draft} options={options} onChange={edit} onDone={closeValues} />
            </PopoverContent>
          ) : null}
        </Popover>
      ) : null}
      <ChipButton aria-label={`Remove ${meta.label.toLowerCase()} filter`} className="w-6 justify-center px-0 text-muted-foreground" onClick={onRemove}>
        <X className="size-3" />
      </ChipButton>
    </FilterChip>
  )
}

/** `Field · operator · values ×` pill: one bordered box, its parts split by a hairline. */
function FilterChip({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="filter-chip" className={cn('inline-flex h-7 max-w-full origin-left items-stretch overflow-hidden rounded-md border bg-background text-xs', className)} {...props} />
}

/** A static part of a filter chip (the field name, a fixed operator). */
function ChipSegment({ className, ...props }: ComponentProps<'span'>) {
  return <span data-slot="filter-chip-segment" className={cn('inline-flex min-w-0 items-center gap-1.5 px-2 whitespace-nowrap text-muted-foreground not-first:border-l', className)} {...props} />
}

/** A clickable part of a filter chip: a square-cornered ghost button with the hairline on its left. */
function ChipButton({ className, ...props }: ComponentProps<typeof Button>) {
  return <Button type="button" variant="ghost" className={cn('h-auto min-w-0 gap-1.5 rounded-none border-0 border-l border-l-border px-2 text-xs font-normal', className)} {...props} />
}

function AdvancedChip({ count, enter, onOpen }: { count: number; enter: boolean; onOpen?: () => void }) {
  const [animate] = useState(enter)
  return (
    <Button type="button" variant="outline" size="sm" disabled={!onOpen} onClick={onOpen} className={cn('origin-left text-xs', animate && 'animate-filter-chip-enter')}>
      <Hierarchy className="text-muted-foreground" aria-hidden="true" />
      {`Advanced filter · ${count} ${count === 1 ? 'condition' : 'conditions'}`}
    </Button>
  )
}
