import { useCommand, useShortcutLabel } from '@/shortcuts/useCommand'
import { useState, type ReactNode } from 'react'
import { ArrowDown, ArrowUp, Diamonds, Setting4 } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tip } from '@/components/common/Tip'
import type { TaskField } from '@orbit/api/generated/types.gen'
import { hidesTaskKey } from '@orbit/api/taskProperties'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { storesValues } from '@/features/tasks/fieldsLib'
import { emptyFilter, PROJECT_GROUPS, TASK_PROPERTIES, viewStatesEqual, type DisplayOptions, type GroupBy, type Layout, type OrderBy, type ShowCompleted, type SubIssuesMode, type TaskProperty } from '../viewState'
import { GROUP_LABEL, LAYOUTS, TIMELINE_PROPERTIES } from '../displayMeta'

export interface DisplayPopoverProps {
  display: DisplayOptions
  /** What "Reset to default" restores: DEFAULT_DISPLAY on pages, the saved display on a view. */
  defaultDisplay: DisplayOptions
  onChange: (patch: Partial<DisplayOptions>) => void
  /** A single project's page: it also groups by milestone and cycle. */
  projectPage?: boolean
  /** Custom task fields offered as properties and orderings (`displayFields`). */
  fields?: readonly TaskField[]
}

const NO_FIELDS: readonly TaskField[] = []

/** Toggle and select values of a custom field, apart from the built-in property and ordering names. */
const fieldKey = (id: string) => `field:${id}`
const fieldIdOf = (key: string) => (key.startsWith('field:') ? key.slice('field:'.length) : null)

const GROUP_ORDER: GroupBy[] = ['status', 'assignee', 'priority', 'project', 'label', 'milestone', 'cycle', 'none']

const ORDER_LABEL: Record<OrderBy, string> = {
  manual: 'Manual',
  priority: 'Priority',
  created: 'Created',
  updated: 'Updated',
  title: 'Title',
  due_date: 'Due date',
  estimate: 'Estimate',
  custom_field: 'Custom field',
}
const ORDER_OPTIONS: OrderBy[] = ['manual', 'priority', 'created', 'updated', 'title', 'due_date', 'estimate']

const COMPLETED_LABEL: Record<ShowCompleted, string> = { all: 'All', past_week: 'Past week', past_month: 'Past month', none: 'None' }
const COMPLETED_OPTIONS: ShowCompleted[] = ['all', 'past_week', 'past_month', 'none']

const PROPERTY_LABEL: Record<TaskProperty, string> = {
  id: 'ID',
  status: 'Status',
  assignee: 'Assignee',
  priority: 'Priority',
  project: 'Project',
  milestone: 'Milestone',
  cycle: 'Cycle',
  estimate: 'Estimate',
  due_date: 'Due date',
  labels: 'Labels',
  created: 'Created',
  updated: 'Updated',
  sub_issue_progress: 'Sub-issue progress',
}

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
export function DisplayPopover({ display, defaultDisplay, onChange, projectPage = false, fields = NO_FIELDS }: DisplayPopoverProps) {
  const [open, setOpen] = useState(false)
  const [instant, setInstant] = useState(false)
  useCommand('view.display', () => {
    setInstant(true)
    setOpen(true)
  })
  const keyShortcuts = useShortcutLabel('view.display')

  const setLayout = (layout: Layout) => onChange(layout === 'board' && display.group_by === 'none' ? { layout, group_by: 'status' } : { layout })
  useCommand('view.layout', () => {
    const layouts = LAYOUTS.map((layout) => layout.value)
    setLayout(layouts[(layouts.indexOf(display.layout) + 1) % layouts.length])
  })
  const setGroup = (group_by: GroupBy) =>
    onChange(group_by === 'none' || group_by === display.sub_group_by ? { group_by, sub_group_by: 'none' } : { group_by })
  const { workspace } = useWorkspace()
  // the built-in properties the workspace hides (Settings, Task fields) are not offered
  const offered = <T extends string>(key: T) => !hidesTaskKey(workspace, key)
  const groups = GROUP_ORDER.filter((group) => (projectPage || !PROJECT_GROUPS.includes(group)) && offered(group))
  const groupItems = groups.filter((group) => display.layout !== 'board' || group !== 'none').map((group) => ({ value: group, label: GROUP_LABEL[group] }))
  const subItems = groups.filter((group) => group !== display.group_by).map((group) => ({
    value: group,
    label: group === 'none' ? 'No sub-grouping' : GROUP_LABEL[group],
  }))
  const timeline = display.layout === 'timeline'
  // the milestone view: the project's timeline, one section per milestone, with target markers
  const milestoneTimeline = timeline && display.group_by === 'milestone'
  const flatOnly = display.layout !== 'list' && display.sub_issues === 'nested'
  const manual = display.order_by === 'manual'
  const ascending = display.order_direction === 'asc'
  const shownProperties = TASK_PROPERTIES.filter((property) => (!timeline || TIMELINE_PROPERTIES.includes(property)) && offered(property))
  // a timeline row has no room for custom fields; their choice stays for list and board
  const shownFields = timeline ? NO_FIELDS : fields
  const customFields = display.custom_fields ?? []
  const orderKey = display.order_by === 'custom_field' && display.order_field_id ? fieldKey(display.order_field_id) : display.order_by
  const orderItems = [
    ...ORDER_OPTIONS.filter(offered).map((order) => ({ value: order as string, label: ORDER_LABEL[order] })),
    ...fields.filter((field) => storesValues(field.kind)).map((field) => ({ value: fieldKey(field.id), label: field.name })),
  ]
  if (!orderItems.some((item) => item.value === orderKey)) orderItems.push({ value: orderKey, label: 'Unknown field' })
  const setOrder = (key: string) => {
    const fieldId = fieldIdOf(key)
    onChange(fieldId ? { order_by: 'custom_field', order_field_id: fieldId } : { order_by: key as OrderBy, order_field_id: undefined })
  }
  const setProperties = (value: string[]) => {
    // a chip hidden on this layout keeps its value
    const properties = TASK_PROPERTIES.filter((property) => (shownProperties.includes(property) ? value.includes(property) : display.properties.includes(property)))
    if (timeline) return onChange({ properties })
    const chosen = value.flatMap((key) => fieldIdOf(key) ?? [])
    // fields keep the order they were turned on in; a deleted field drops out
    const kept = customFields.filter((id) => chosen.includes(id) && fields.some((field) => field.id === id))
    onChange({ properties, custom_fields: [...kept, ...chosen.filter((id) => !kept.includes(id))] })
  }

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
          <Button type="button" variant="ghost" aria-label="Display options" aria-keyshortcuts={keyShortcuts} className="max-[899px]:w-8 max-[899px]:px-0">
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
          {projectPage && !milestoneTimeline && offered('milestone') ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="mt-2 w-full justify-start font-normal text-muted-foreground"
              onClick={() => onChange({ layout: 'timeline', group_by: 'milestone', ...(display.sub_group_by === 'milestone' && { sub_group_by: 'none' }) })}
            >
              <Diamonds className="size-3.5" aria-hidden="true" />
              Milestone timeline
            </Button>
          ) : null}
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
                <OptionSelect label="Ordering" value={orderKey} items={orderItems} onChange={setOrder} />
                <Tip label={manual ? 'Manual order has no direction' : ascending ? 'Ascending' : 'Descending'}>
                  <Button
                    type="button"
                    variant="outline"
                    size="icon-sm"
                    aria-label={ascending ? 'Ascending' : 'Descending'}
                    disabled={manual}
                    onClick={() => onChange({ order_direction: ascending ? 'desc' : 'asc' })}
                  >
                    {ascending ? <ArrowUp /> : <ArrowDown />}
                  </Button>
                </Tip>
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
            value={[...display.properties, ...customFields.map(fieldKey)]}
            onValueChange={setProperties}
          >
            {shownProperties.map((property) => (
              <ToggleGroupItem key={property} value={property} className="h-6 min-w-0 px-2 text-xs font-normal text-muted-foreground aria-pressed:text-foreground">
                {PROPERTY_LABEL[property]}
              </ToggleGroupItem>
            ))}
            {shownFields.map((field) => (
              <ToggleGroupItem key={field.id} value={fieldKey(field.id)} title={field.name} className="h-6 max-w-40 min-w-0 px-2 text-xs font-normal text-muted-foreground aria-pressed:text-foreground">
                <span className="truncate">{field.name}</span>
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>
        <div className="flex justify-end border-t p-2">
          <Button type="button" variant="ghost" size="sm" disabled={sameDisplay(display, defaultDisplay)} onClick={() => onChange({ custom_fields: undefined, order_field_id: undefined, ...defaultDisplay })}>
            Reset to default
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}
