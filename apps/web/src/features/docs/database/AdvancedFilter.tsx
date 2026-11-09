import { useState, type ReactNode } from 'react'
import { Add, Copy, MoreH, Trash } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { filterRules, isGroup, MAX_GROUP_CHILDREN, MAX_RULES } from '@orbit/database/filter'
import { appendChild, canNestGroup, duplicateNode, updateNode, wrapInGroup, type NodePath } from '@orbit/database/filterTree'
import { newRule, OPERATOR_LABELS, operatorsOf, ruleNeedsValue, ruleValueText } from '@orbit/database/meta'
import type { DatabaseFilterGroup, DatabaseFilterNode, DatabaseFilterOperator, DatabaseFilterRule, Property } from '@orbit/database/types'
import { useDatabaseEnv } from './context'
import { RuleValue } from './FilterBar'

const OPS = [
  { value: 'and', label: 'And' },
  { value: 'or', label: 'Or' },
]

interface AdvancedFilterProps {
  properties: Property[]
  filter: DatabaseFilterGroup
  onChange: (filter: DatabaseFilterGroup) => void
}

/**
 * The advanced filter (spec P2 5.4), laid out as the task advanced filter: one line per rule or group, `Where`, then the
 * group's `And`/`Or` (the second line sets it), property, operator, value and "⋯". Groups nest 3 levels.
 */
export function AdvancedFilter({ properties, filter, onChange }: AdvancedFilterProps) {
  const rulesLeft = MAX_RULES - filterRules(filter).length
  return (
    <div className="flex flex-col gap-2">
      {filter.children.length === 0 ? <p className="px-1 text-sm text-muted-foreground">No filter rules. The view shows every row.</p> : null}
      <GroupLines group={filter} path={[]} root={filter} properties={properties} rulesLeft={rulesLeft} onChange={onChange} />
    </div>
  )
}

interface LineProps {
  path: NodePath
  root: DatabaseFilterGroup
  properties: Property[]
  /** Rules the filter can still take (it holds at most 20). */
  rulesLeft: number
  onChange: (filter: DatabaseFilterGroup) => void
}

function GroupLines({ group, path, root, properties, rulesLeft, onChange }: LineProps & { group: DatabaseFilterGroup }) {
  const roomy = group.children.length < MAX_GROUP_CHILDREN && rulesLeft > 0
  const first = properties[0]
  return (
    <div className="flex flex-col gap-1.5">
      {group.children.map((child, index) => (
        <div key={index} className="flex items-start gap-1.5">
          <div className="flex h-7 w-16 shrink-0 items-center text-sm text-muted-foreground">
            {index === 0 ? (
              <span className="px-1">Where</span>
            ) : index === 1 ? (
              <Select items={OPS} value={group.op} onValueChange={(op) => op && onChange(updateNode(root, path, (node) => (isGroup(node) ? { ...node, op: op as DatabaseFilterGroup['op'] } : node)))}>
                <SelectTrigger size="sm" aria-label="Match" className="w-16 px-1.5">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {OPS.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <span className="px-1">{group.op === 'and' ? 'And' : 'Or'}</span>
            )}
          </div>
          <NodeLine node={child} path={[...path, index]} root={root} properties={properties} rulesLeft={rulesLeft} roomy={roomy} onChange={onChange} />
        </div>
      ))}
      {first ? (
        <div className="flex gap-1">
          <Button type="button" variant="ghost" size="sm" className="text-muted-foreground" disabled={!roomy} onClick={() => onChange(appendChild(root, path, newRule(first)))}>
            <Add />
            Add filter rule
          </Button>
          {canNestGroup(path) ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="text-muted-foreground"
              disabled={!roomy}
              onClick={() => onChange(appendChild(root, path, { op: group.op === 'and' ? 'or' : 'and', children: [newRule(first)] }))}
            >
              <Add />
              Add filter group
            </Button>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

function NodeLine({ node, path, root, properties, rulesLeft, roomy, onChange }: LineProps & { node: DatabaseFilterNode; roomy: boolean }) {
  const parent = path.slice(0, -1)
  const copies = isGroup(node) ? filterRules(node).length : 1
  const menu = (
    <LineMenu
      label={isGroup(node) ? 'Group' : 'Rule'}
      onRemove={() => onChange(updateNode(root, path, () => null))}
      onDuplicate={roomy && copies <= rulesLeft ? () => onChange(duplicateNode(root, path)) : undefined}
      onGroup={!isGroup(node) && canNestGroup(parent) ? () => onChange(wrapInGroup(root, path)) : undefined}
    />
  )
  if (isGroup(node)) {
    return (
      <div role="group" aria-label="Filter group" className="flex min-w-0 flex-1 items-start gap-1 rounded-md border border-border bg-muted/30 p-1.5">
        <div className="min-w-0 flex-1">
          <GroupLines group={node} path={path} root={root} properties={properties} rulesLeft={rulesLeft} onChange={onChange} />
        </div>
        {menu}
      </div>
    )
  }
  return (
    <div role="group" aria-label="Filter rule" className="flex min-w-0 flex-1 items-center gap-1.5">
      <RuleFields rule={node} properties={properties} onChange={(rule) => onChange(updateNode(root, path, () => rule))} />
      {menu}
    </div>
  )
}

function RuleFields({ rule, properties, onChange }: { rule: DatabaseFilterRule; properties: Property[]; onChange: (rule: DatabaseFilterRule) => void }) {
  const property = properties.find((candidate) => candidate.id === rule.property_id)
  if (!property) return null
  const propertyItems = properties.map((candidate) => ({ value: candidate.id, label: candidate.name }))
  const operatorItems = operatorsOf(property.type).map((operator) => ({ value: operator, label: OPERATOR_LABELS[operator] }))
  const setOperator = (operator: DatabaseFilterOperator) => onChange({ ...rule, operator, value: ruleNeedsValue(operator) ? rule.value : null })
  return (
    <>
      <Select
        items={propertyItems}
        value={property.id}
        onValueChange={(id) => {
          const next = properties.find((candidate) => candidate.id === id)
          if (next && next.id !== property.id) onChange(newRule(next))
        }}
      >
        <SelectTrigger size="sm" aria-label="Property" className="w-36">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {propertyItems.map((item) => (
            <SelectItem key={item.value} value={item.value}>
              {item.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select items={operatorItems} value={rule.operator} onValueChange={(value) => value && setOperator(value as DatabaseFilterOperator)}>
        <SelectTrigger size="sm" aria-label="Operator" className="w-36">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {operatorItems.map((item) => (
            <SelectItem key={item.value} value={item.value}>
              {item.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {ruleNeedsValue(rule.operator) ? <ValueField rule={rule} property={property} onChange={(value) => onChange({ ...rule, value })} /> : null}
    </>
  )
}

/** Text and numbers edit in the line; the other values in a popover, as the chips' rule editor. */
function ValueField({ rule, property, onChange }: { rule: DatabaseFilterRule; property: Property; onChange: (value: unknown) => void }) {
  const { ctx } = useDatabaseEnv()
  if (property.type === 'title' || property.type === 'text' || property.type === 'url') {
    return <Input aria-label="Value" placeholder="Value" className="h-7 w-40" value={typeof rule.value === 'string' ? rule.value : ''} onChange={(event) => onChange(event.target.value)} />
  }
  if (property.type === 'number') {
    return (
      <Input
        type="number"
        aria-label="Value"
        placeholder="Value"
        className="h-7 w-28"
        value={typeof rule.value === 'number' ? String(rule.value) : ''}
        onChange={(event) => onChange(event.target.value === '' ? null : Number(event.target.value))}
      />
    )
  }
  const text = ruleValueText(rule, property, ctx)
  return (
    <ValuePopover label={text ?? 'Value'} empty={text === null}>
      <RuleValue rule={rule} property={property} onChange={onChange} />
    </ValuePopover>
  )
}

function ValuePopover({ label, empty, children }: { label: string; empty: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button type="button" variant="outline" size="sm" aria-label={`Value: ${label}`} data-empty={empty || undefined} className="h-7 w-40 justify-start truncate font-normal data-empty:text-muted-foreground" />
        }
      >
        <span className="truncate">{label}</span>
      </PopoverTrigger>
      {open ? (
        <PopoverContent align="start" className="w-72 gap-2 p-2">
          {children}
        </PopoverContent>
      ) : null}
    </Popover>
  )
}

function LineMenu({ label, onRemove, onDuplicate, onGroup }: { label: string; onRemove: () => void; onDuplicate?: () => void; onGroup?: () => void }) {
  const [open, setOpen] = useState(false)
  return (
    <DropdownMenu open={open} onOpenChange={setOpen} modal={false}>
      <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon-sm" aria-label={`${label} actions`} className="size-7 shrink-0 text-muted-foreground" />}>
        <MoreH />
      </DropdownMenuTrigger>
      {open ? (
        <DropdownMenuContent align="end" className="w-44">
          <DropdownMenuItem onClick={onRemove}>
            <Trash />
            Remove
          </DropdownMenuItem>
          {onDuplicate ? (
            <DropdownMenuItem onClick={onDuplicate}>
              <Copy />
              Duplicate
            </DropdownMenuItem>
          ) : null}
          {onGroup ? (
            <DropdownMenuItem onClick={onGroup}>
              <Add />
              Turn into group
            </DropdownMenuItem>
          ) : null}
        </DropdownMenuContent>
      ) : null}
    </DropdownMenu>
  )
}
