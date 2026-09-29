import { useEffect, useState, type ReactNode } from 'react'
import { ArrowDown, ArrowUp, Setting4 } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { shouldIgnoreShortcut } from '../shortcuts'
import { emptyFilter, viewStatesEqual, type DisplayOptions, type GroupBy, type Layout, type OrderBy, type ShowCompleted, type SubIssuesMode, type TaskProperty } from '../viewState'
import { GROUP_LABEL, LAYOUTS, TIMELINE_PROPERTIES } from '../displayMeta'

export interface DisplayPopoverProps {
  display: DisplayOptions
  /** What "Reset to default" restores: DEFAULT_DISPLAY on pages, the saved display on a view. */
  defaultDisplay: DisplayOptions
  onChange: (patch: Partial<DisplayOptions>) => void
}

const GROUP_ORDER: GroupBy[] = ['status', 'assignee', 'priority', 'project', 'label', 'none']

const ORDER_LABEL: Record<OrderBy, string> = {
  manual: 'Manual',
  priority: 'Priority',
  created: 'Created',
  updated: 'Updated',
  title: 'Title',
  due_date: 'Due date',
}
const ORDER_OPTIONS: OrderBy[] = ['manual', 'priority', 'created', 'updated', 'title', 'due_date']

const COMPLETED_LABEL: Record<ShowCompleted, string> = { all: 'All', past_week: 'Past week', past_month: 'Past month', none: 'None' }
const COMPLETED_OPTIONS: ShowCompleted[] = ['all', 'past_week', 'past_month', 'none']

const PROPERTY_LABEL: Record<TaskProperty, string> = {
  id: 'ID',
  status: 'Status',
  assignee: 'Assignee',
  priority: 'Priority',
  project: 'Project',
  due_date: 'Due date',
  labels: 'Labels',
  created: 'Created',
  updated: 'Updated',
  sub_issue_progress: 'Sub-issue progress',
}
const PROPERTY_ORDER: TaskProperty[] = ['id', 'status', 'assignee', 'priority', 'project', 'due_date', 'labels', 'created', 'updated', 'sub_issue_progress']

const SUB_ISSUE_LABEL: Record<SubIssuesMode, string> = { nested: 'Nested', flat: 'Flat', hidden: 'Hidden' }
const SUB_ISSUE_OPTIONS: SubIssuesMode[] = ['nested', 'flat', 'hidden']

/** Semantic equality (properties are a set, normalization applied), reusing the saved-view comparison. */
function sameDisplay(a: DisplayOptions, b: DisplayOptions): boolean {
  return viewStatesEqual({ filter: emptyFilter(), display: a }, { filter: emptyFilter(), display: b })
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex h-8 items-center justify-between gap-3 text-sm">
      <span className="text-muted-foreground">{label}</span>
      {children}
    </div>
  )
}

function OptionSelect<T extends string>({ label, value, items, onChange, describedBy }: { label: string; value: T; items: Array<{ value: T; label: string }>; onChange: (value: T) => void; describedBy?: string }) {
  return (
    <Select
      items={items}
      value={value}
      onValueChange={(next) => {
        if (typeof next === 'string') onChange(next as T)
      }}
    >
      <SelectTrigger size="sm" className="w-40" aria-label={label} aria-describedby={describedBy}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {items.map((item) => (
          <SelectItem key={item.value} value={item.value}>
            {item.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

/** Header "Display" button. `Shift+V` opens it (never while typing); keyboard opens skip the entrance animation. */
export function DisplayPopover({ display, defaultDisplay, onChange }: DisplayPopoverProps) {
  const [open, setOpen] = useState(false)
  const [instant, setInstant] = useState(false)
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.shiftKey || (event.key !== 'V' && event.key !== 'v')) return
      if (shouldIgnoreShortcut(event)) return
      event.preventDefault()
      setInstant(true)
      setOpen(true)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])

  const setLayout = (layout: Layout) => onChange(layout === 'board' && display.group_by === 'none' ? { layout, group_by: 'status' } : { layout })
  const setGroup = (group_by: GroupBy) =>
    onChange(group_by === 'none' || group_by === display.sub_group_by ? { group_by, sub_group_by: 'none' } : { group_by })
  const groupItems = GROUP_ORDER.filter((group) => display.layout !== 'board' || group !== 'none').map((group) => ({ value: group, label: GROUP_LABEL[group] }))
  const subItems = GROUP_ORDER.filter((group) => group !== display.group_by).map((group) => ({
    value: group,
    label: group === 'none' ? 'No sub-grouping' : GROUP_LABEL[group],
  }))
  const timeline = display.layout === 'timeline'
  const flatOnly = display.layout !== 'list' && display.sub_issues === 'nested'
  const manual = display.order_by === 'manual'
  const ascending = display.order_direction === 'asc'
  const shownProperties = timeline ? PROPERTY_ORDER.filter((property) => TIMELINE_PROPERTIES.includes(property)) : PROPERTY_ORDER

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) setInstant(false)
      }}
      modal={false}
    >
      <PopoverTrigger
        render={
          <Button type="button" variant="ghost" aria-label="Display options" aria-keyshortcuts="Shift+V" className="max-[899px]:w-8 max-[899px]:px-0">
            <Setting4 className="size-4" />
            <span className="max-[899px]:hidden">Display</span>
          </Button>
        }
      />
      <PopoverContent align="end" {...(instant && { 'data-instant': '' })} className="w-80 gap-0 p-0 data-instant:animate-none">
        <div className="p-3">
          <ToggleGroup
            aria-label="Layout"
            variant="outline"
            className="w-full"
            value={[display.layout]}
            onValueChange={(value: string[]) => {
              const next = value[0]
              if (next === 'list' || next === 'board' || next === 'timeline') setLayout(next)
            }}
          >
            {LAYOUTS.map(({ value, label, icon: Icon }) => (
              <ToggleGroupItem key={value} value={value} className="flex-1 gap-1.5 font-normal text-muted-foreground aria-pressed:text-foreground">
                <Icon className="size-3.5" aria-hidden="true" />
                {label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>
        <div className="flex flex-col gap-1 border-t p-3">
          <Row label="Grouping">
            <OptionSelect label="Grouping" value={display.group_by} items={groupItems} onChange={setGroup} />
          </Row>
          {!timeline && display.group_by !== 'none' ? (
            <Row label="Sub-grouping">
              <OptionSelect label="Sub-grouping" value={display.sub_group_by} items={subItems} onChange={(sub_group_by) => onChange({ sub_group_by })} />
            </Row>
          ) : null}
          {/* Timeline rows always follow their dates, so an ordering would do nothing there. */}
          {timeline ? null : (
            <Row label="Ordering">
              <div className="flex items-center gap-1">
                <OptionSelect
                  label="Ordering"
                  value={display.order_by}
                  items={ORDER_OPTIONS.map((order) => ({ value: order, label: ORDER_LABEL[order] }))}
                  onChange={(order_by) => onChange({ order_by })}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="icon-sm"
                  aria-label={ascending ? 'Ascending' : 'Descending'}
                  disabled={manual}
                  title={manual ? 'Manual order has no direction' : undefined}
                  onClick={() => onChange({ order_direction: ascending ? 'desc' : 'asc' })}
                >
                  {ascending ? <ArrowUp /> : <ArrowDown />}
                </Button>
              </div>
            </Row>
          )}
          <Row label="Sub-issues">
            <OptionSelect
              label="Sub-issues"
              value={display.sub_issues}
              items={SUB_ISSUE_OPTIONS.map((mode) => ({ value: mode, label: SUB_ISSUE_LABEL[mode] }))}
              onChange={(sub_issues) => onChange({ sub_issues })}
              describedBy={flatOnly ? 'display-sub-issues-hint' : undefined}
            />
          </Row>
          {/* Board and timeline render Nested as Flat (spec §7.1): the value stays (it is saved with the view for the list) */}
          {flatOnly ? (
            <p id="display-sub-issues-hint" className="-mt-1 mb-1 w-40 self-end text-xs text-muted-foreground">
              {timeline ? 'Timeline' : 'Board'} shows sub-issues flat
            </p>
          ) : null}
          <Row label="Completed tasks">
            <OptionSelect
              label="Completed tasks"
              value={display.show_completed}
              items={COMPLETED_OPTIONS.map((option) => ({ value: option, label: COMPLETED_LABEL[option] }))}
              onChange={(show_completed) => onChange({ show_completed })}
            />
          </Row>
          <Row label="Show empty groups">
            {/* the thumb snaps: nothing in this everyday panel moves except the entrance */}
            <Switch
              aria-label="Show empty groups"
              className="[&_[data-slot=switch-thumb]]:transition-none"
              checked={display.show_empty_groups}
              onCheckedChange={(checked: boolean) => onChange({ show_empty_groups: checked })}
            />
          </Row>
        </div>
        <div className="flex flex-col gap-2 border-t p-3">
          <span className="text-xs font-medium text-muted-foreground">Display properties</span>
          <ToggleGroup
            aria-label="Display properties"
            multiple
            variant="outline"
            size="sm"
            spacing={1}
            className="flex-wrap justify-start"
            value={display.properties}
            // a chip hidden on this layout keeps its value
            onValueChange={(value: string[]) =>
              onChange({ properties: PROPERTY_ORDER.filter((property) => (shownProperties.includes(property) ? value.includes(property) : display.properties.includes(property))) })}
          >
            {shownProperties.map((property) => (
              <ToggleGroupItem key={property} value={property} className="h-6 min-w-0 px-2 text-xs font-normal text-muted-foreground aria-pressed:text-foreground">
                {PROPERTY_LABEL[property]}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>
        <div className="flex justify-end border-t p-2">
          <Button type="button" variant="ghost" size="sm" disabled={sameDisplay(display, defaultDisplay)} onClick={() => onChange(defaultDisplay)}>
            Reset to default
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}
