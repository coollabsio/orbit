import { useState, type ComponentProps, type ReactElement } from 'react'
import { Add, Xmark } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { UserAvatar } from '@/components/common/UserAvatar'
import { options } from '@orbit/database/values'
import type { DatabaseFilterOperator, DatabaseFilterRule, Property, ViewConfig } from '@orbit/database/types'
import { Pill } from './cells/Pill'
import { useDatabaseEnv } from './context'
import { OPERATOR_LABELS, RELATIVE_DATES, newRule, operatorsOf, ruleNeedsValue, ruleSummary } from '@orbit/database/meta'
import { PROPERTY_TYPES } from './propertyMeta'

const MAX_RULES = 20

interface FilterBarProps {
  properties: Property[]
  config: ViewConfig
  onConfig: (config: ViewConfig) => void
  /** The rule whose editor is open (a rule just added opens at once). */
  openRule: number | null
  onOpenRule: (index: number | null) => void
}

/** One chip per filter rule and "+ Add filter" (spec 7.1). Shows only when the view has rules. */
export function FilterBar({ properties, config, onConfig, openRule, onOpenRule }: FilterBarProps) {
  const rules = config.filter.rules
  if (rules.length === 0) return null
  const byId = new Map(properties.map((property) => [property.id, property]))
  const setRules = (next: DatabaseFilterRule[]) => onConfig({ ...config, filter: { rules: next } })
  return (
    <div role="toolbar" aria-label="Filters" className="flex flex-wrap items-center gap-1.5 py-1.5">
      {rules.map((rule, index) => {
        const property = byId.get(rule.property_id)
        if (!property) return null
        return (
          <RuleChip
            key={`${index}:${rule.property_id}`}
            rule={rule}
            property={property}
            open={openRule === index}
            onOpenChange={(open) => onOpenRule(open ? index : null)}
            onChange={(next) => setRules(rules.map((other, at) => (at === index ? next : other)))}
            onRemove={() => {
              onOpenRule(null)
              setRules(rules.filter((_, at) => at !== index))
            }}
          />
        )
      })}
      {rules.length < MAX_RULES ? (
        <PropertyPicker
          properties={properties}
          label="Add filter"
          onPick={(property) => {
            setRules([...rules, newRule(property)])
            onOpenRule(rules.length)
          }}
          trigger={
            <Button type="button" variant="ghost" size="sm" className="text-muted-foreground">
              <Add />
              Add filter
            </Button>
          }
        />
      ) : null}
    </div>
  )
}

/** A searchable list of the properties; picking one calls `onPick`. */
export function PropertyPicker({ properties, label, onPick, trigger, open: controlledOpen, onOpenChange }: {
  properties: Property[]
  label: string
  onPick: (property: Property) => void
  trigger: ReactElement
  open?: boolean
  onOpenChange?: (open: boolean) => void
}) {
  const [ownOpen, setOwnOpen] = useState(false)
  const open = controlledOpen ?? ownOpen
  const setOpen = onOpenChange ?? setOwnOpen
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger render={trigger} />
      <PopoverContent align="start" className="w-60 gap-0 p-0">
        <Command label={label} loop>
          <CommandInput autoFocus aria-label={label} placeholder="Property…" />
          <CommandList>
            <CommandEmpty className="py-3 text-center text-xs text-muted-foreground">No properties</CommandEmpty>
            <CommandGroup>
              {properties.map((property) => {
                const Icon = PROPERTY_TYPES[property.type].icon
                return (
                  <CommandItem
                    key={property.id}
                    value={property.id}
                    keywords={[property.name]}
                    onSelect={() => {
                      setOpen(false)
                      onPick(property)
                    }}
                  >
                    <Icon className="text-muted-foreground" />
                    <span className="truncate">{property.name}</span>
                  </CommandItem>
                )
              })}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

function FilterChip({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="filter-chip" className={cn('inline-flex h-7 max-w-full items-stretch overflow-hidden rounded-md border border-border bg-background text-xs', className)} {...props} />
}

function RuleChip({ rule, property, open, onOpenChange, onChange, onRemove }: {
  rule: DatabaseFilterRule
  property: Property
  open: boolean
  onOpenChange: (open: boolean) => void
  onChange: (rule: DatabaseFilterRule) => void
  onRemove: () => void
}) {
  const { ctx } = useDatabaseEnv()
  const Icon = PROPERTY_TYPES[property.type].icon
  const summary = ruleSummary(rule, property, ctx)
  return (
    <FilterChip data-filter-chip={property.id}>
      <Popover open={open} onOpenChange={onOpenChange}>
        <PopoverTrigger
          render={
            <Button
              type="button"
              variant="ghost"
              className="h-auto min-w-0 gap-1.5 rounded-none border-0 px-2 text-xs font-normal data-active:text-primary"
              data-active={summary !== 'All' || undefined}
              aria-label={`${property.name} filter: ${summary}`}
            />
          }
        >
          <Icon className="size-3.5" aria-hidden="true" />
          <span className="font-medium">{property.name}:</span>
          <span className="max-w-56 truncate">{summary}</span>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-72 gap-2 p-2">
          <RuleEditor rule={rule} property={property} onChange={onChange} />
        </PopoverContent>
      </Popover>
      <Button
        type="button"
        variant="ghost"
        aria-label={`Remove ${property.name} filter`}
        className="h-auto w-6 rounded-none border-0 border-l border-l-border px-0 text-muted-foreground"
        onClick={onRemove}
      >
        <Xmark className="size-3" />
      </Button>
    </FilterChip>
  )
}

/** The operator select and the value editor of the property's type. */
function RuleEditor({ rule, property, onChange }: { rule: DatabaseFilterRule; property: Property; onChange: (rule: DatabaseFilterRule) => void }) {
  const operators = operatorsOf(property.type)
  const items = operators.map((operator) => ({ value: operator, label: OPERATOR_LABELS[operator] }))
  const needsValue = ruleNeedsValue(rule.operator)
  const setOperator = (operator: DatabaseFilterOperator) =>
    onChange({ ...rule, operator, value: ruleNeedsValue(operator) ? rule.value : null })
  return (
    <>
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-sm text-muted-foreground">{property.name}</span>
        {operators.length > 1 ? (
          <Select items={items} value={rule.operator} onValueChange={(value) => value && setOperator(value as DatabaseFilterOperator)}>
            <SelectTrigger size="sm" aria-label="Operator" className="w-auto shrink-0">
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
        ) : null}
      </div>
      {needsValue ? <RuleValue rule={rule} property={property} onChange={(value) => onChange({ ...rule, value })} /> : null}
    </>
  )
}

function RuleValue({ rule, property, onChange }: { rule: DatabaseFilterRule; property: Property; onChange: (value: unknown) => void }) {
  const { users } = useDatabaseEnv()
  const value = rule.value
  switch (property.type) {
    case 'title':
    case 'text':
    case 'url':
      return (
        <Input autoFocus aria-label="Value" placeholder="Value" value={typeof value === 'string' ? value : ''} onChange={(event) => onChange(event.target.value)} />
      )
    case 'number':
      return (
        <Input
          autoFocus
          type="number"
          aria-label="Value"
          placeholder="Value"
          value={typeof value === 'number' ? String(value) : ''}
          onChange={(event) => onChange(event.target.value === '' ? null : Number(event.target.value))}
        />
      )
    case 'checkbox':
      return (
        <div className="flex gap-1">
          {[true, false].map((checked) => (
            <Button key={String(checked)} type="button" variant={value === checked ? 'secondary' : 'ghost'} size="sm" onClick={() => onChange(checked)}>
              {checked ? 'Checked' : 'Unchecked'}
            </Button>
          ))}
        </div>
      )
    case 'select':
    case 'status':
    case 'multi_select':
      return (
        <IdList
          ids={Array.isArray(value) ? (value as string[]) : []}
          onChange={onChange}
          items={options(property).map((option) => ({
            id: option.id,
            keywords: [option.name],
            label: <Pill color={option.color} dot={property.type === 'status'}>{option.name}</Pill>,
          }))}
        />
      )
    case 'person':
    case 'created_by':
    case 'last_edited_by':
      return (
        <IdList
          ids={Array.isArray(value) ? (value as string[]) : []}
          onChange={onChange}
          items={[
            { id: 'me', keywords: ['Me'], label: <span>Me</span> },
            ...[...users.values()].map((user) => ({
              id: user.id,
              keywords: [user.name, user.email],
              label: (
                <span className="flex min-w-0 items-center gap-1.5">
                  <UserAvatar user={user} size={18} />
                  <span className="truncate">{user.name}</span>
                </span>
              ),
            })),
          ]}
        />
      )
    case 'date':
    case 'created_time':
    case 'last_edited_time': {
      const relative = typeof value === 'string' && RELATIVE_DATES.some((date) => date.value === value)
      const items = [...RELATIVE_DATES, { value: 'exact', label: 'Exact date' }]
      return (
        <div className="flex flex-col gap-2">
          <Select items={items} value={relative ? (value as string) : 'exact'} onValueChange={(next) => next && onChange(next === 'exact' ? null : next)}>
            <SelectTrigger size="sm" aria-label="Date" className="w-full">
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
          {relative ? null : (
            <Input
              type="date"
              aria-label="Exact date"
              value={typeof value === 'string' ? value.slice(0, 10) : ''}
              onChange={(event) => onChange(event.target.value || null)}
            />
          )}
        </div>
      )
    }
  }
}

function IdList({ ids, items, onChange }: { ids: string[]; items: { id: string; keywords: string[]; label: ReactElement }[]; onChange: (ids: string[]) => void }) {
  return (
    <Command label="Values" loop className="rounded-md! border border-border p-0">
      <CommandInput autoFocus aria-label="Search values" placeholder="Search…" />
      <CommandList className="max-h-56">
        <CommandEmpty className="py-3 text-center text-xs text-muted-foreground">Nothing found</CommandEmpty>
        <CommandGroup>
          {items.map((item) => (
            <CommandItem
              key={item.id}
              value={item.id}
              keywords={item.keywords}
              data-checked={ids.includes(item.id) || undefined}
              onSelect={() => onChange(ids.includes(item.id) ? ids.filter((id) => id !== item.id) : [...ids, item.id])}
            >
              {item.label}
            </CommandItem>
          ))}
        </CommandGroup>
      </CommandList>
    </Command>
  )
}
