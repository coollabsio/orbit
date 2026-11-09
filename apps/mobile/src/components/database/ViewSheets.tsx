/**
 * The view sheets of a database screen: the filter rules (the web's `FilterBar` chips and rule editor) and the sorts
 * (the web's `SortPopover`, with up and down instead of drag). Every change goes to `onConfig`.
 */
import { newRule, OPERATOR_LABELS, operatorsOf, RELATIVE_DATES, ruleNeedsValue, ruleSummary } from '@orbit/database/meta'
import type { DatabaseFilterOperator, DatabaseFilterRule, DatabaseViewSort, Property, ViewConfig, ViewContext } from '@orbit/database/types'
import { dayOf, localDate } from '@orbit/database/dates'
import { options } from '@orbit/database/values'
import { type ReactNode, useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import { PROPERTY_ICONS } from '../../data/database'
import { usePeople } from '../../queries'
import { useTheme } from '../../theme'
import { Avatar, Button, Icon, IconButton, Input, Text } from '../../ui'
import { DayPicker } from '../tasks/DayPicker'
import { SheetList, SheetNote, SheetRow, SheetSearch } from '../tasks/Sheet'
import { ChoiceRow, Pill } from './cells'

const MAX_RULES = 20
const MAX_SORTS = 10

interface ConfigProps {
  properties: Property[]
  config: ViewConfig
  onConfig: (config: ViewConfig) => void
}

/** The property list of "Add filter" and "Add sort". */
function PropertyChoices({ properties, onPick }: { properties: readonly Property[]; onPick: (property: Property) => void }) {
  const { colors } = useTheme()
  const [query, setQuery] = useState('')
  const needle = query.trim().toLowerCase()
  const shown = properties.filter((property) => !needle || property.name.toLowerCase().includes(needle))
  return (
    <>
      <SheetSearch value={query} onChange={setQuery} placeholder="Property…" />
      <SheetList>
        {shown.map((property) => (
          <SheetRow key={property.id} label={property.name} leading={<Icon name={PROPERTY_ICONS[property.type]} size={18} color={colors.mutedForeground} />} onPress={() => onPick(property)} />
        ))}
        {shown.length === 0 ? <SheetNote>No properties</SheetNote> : null}
      </SheetList>
    </>
  )
}

/** The rules of the view; a rule opens its editor in place, and "Add filter" picks a property first. */
export function FilterContent({ properties, config, onConfig, ctx, openRule, onOpenRule }: ConfigProps & {
  ctx: ViewContext
  /** The rule whose editor shows; `'add'` shows the property list. */
  openRule: number | 'add' | null
  onOpenRule: (rule: number | 'add' | null) => void
}) {
  const { colors } = useTheme()
  const rules = config.filter.rules
  const byId = new Map(properties.map((property) => [property.id, property]))
  const setRules = (next: DatabaseFilterRule[]) => onConfig({ ...config, filter: { rules: next } })

  if (openRule === 'add') {
    return (
      <PropertyChoices
        properties={properties}
        onPick={(property) => {
          setRules([...rules, newRule(property)])
          onOpenRule(rules.length)
        }}
      />
    )
  }
  const open = openRule === null ? undefined : rules[openRule]
  const openProperty = open ? byId.get(open.property_id) : undefined
  if (open && openProperty && openRule !== null) {
    return (
      <RuleEditor
        rule={open}
        property={openProperty}
        onChange={(next) => setRules(rules.map((other, at) => (at === openRule ? next : other)))}
        onBack={() => onOpenRule(null)}
      />
    )
  }
  return (
    <SheetList>
      {rules.length === 0 ? <SheetNote>No filters. The view shows every row.</SheetNote> : null}
      {rules.map((rule, index) => {
        const property = byId.get(rule.property_id)
        if (!property) return null
        return (
          <View key={`${index}:${rule.property_id}`} className="flex-row items-center pr-2">
            <View className="flex-1">
              <SheetRow
                label={`${property.name}: ${ruleSummary(rule, property, ctx)}`}
                leading={<Icon name={PROPERTY_ICONS[property.type]} size={18} color={colors.mutedForeground} />}
                onPress={() => onOpenRule(index)}
              />
            </View>
            <IconButton icon="close" label={`Remove ${property.name} filter`} color={colors.mutedForeground} onPress={() => setRules(rules.filter((_, at) => at !== index))} />
          </View>
        )
      })}
      {rules.length < MAX_RULES ? <SheetRow label="Add filter" icon="plus" onPress={() => onOpenRule('add')} /> : null}
      {rules.length > 0 ? <SheetRow label="Delete all filters" icon="trash" onPress={() => setRules([])} /> : null}
    </SheetList>
  )
}

function RuleEditor({ rule, property, onChange, onBack }: { rule: DatabaseFilterRule; property: Property; onChange: (rule: DatabaseFilterRule) => void; onBack: () => void }) {
  const operators = operatorsOf(property.type)
  const setOperator = (operator: DatabaseFilterOperator) => onChange({ ...rule, operator, value: ruleNeedsValue(operator) ? rule.value : null })
  return (
    <>
      <View className="flex-row items-center gap-1 px-2 pb-1">
        <Button label="All filters" icon="back" variant="ghost" size="sm" muted onPress={onBack} />
      </View>
      {operators.length > 1 ? (
        <View accessibilityRole="radiogroup" accessibilityLabel="Operator" className="flex-row flex-wrap gap-1.5 px-4 pb-2">
          {operators.map((operator) => (
            <Choice key={operator} label={OPERATOR_LABELS[operator]} selected={rule.operator === operator} onPress={() => setOperator(operator)} />
          ))}
        </View>
      ) : null}
      {ruleNeedsValue(rule.operator) ? <RuleValue rule={rule} property={property} onChange={(value) => onChange({ ...rule, value })} /> : null}
    </>
  )
}

function Choice({ label, selected, onPress, children }: { label: string; selected: boolean; onPress: () => void; children?: ReactNode }) {
  return (
    <Pressable
      accessibilityRole="radio"
      accessibilityLabel={label}
      accessibilityState={{ selected }}
      onPress={onPress}
      className={`h-7 flex-row items-center gap-1.5 px-2.5 rounded-md border ${selected ? 'bg-muted border-input' : 'border-border active:bg-muted'}`}
    >
      {children}
      <Text className={`text-[13px] ${selected ? 'font-noto-medium text-foreground' : 'text-muted-foreground'}`}>{label}</Text>
    </Pressable>
  )
}

function RuleValue({ rule, property, onChange }: { rule: DatabaseFilterRule; property: Property; onChange: (value: unknown) => void }) {
  const people = usePeople()
  const value = rule.value
  switch (property.type) {
    case 'title':
    case 'text':
    case 'url':
      return (
        <View className="px-4 pb-3">
          <Input label="Value" placeholder="Value" value={typeof value === 'string' ? value : ''} onChangeText={onChange} autoCapitalize="none" autoCorrect={false} autoFocus />
        </View>
      )
    case 'number':
      return (
        <View className="px-4 pb-3">
          <NumberValue value={typeof value === 'number' ? value : null} onChange={onChange} />
        </View>
      )
    case 'checkbox':
      return (
        <View className="flex-row gap-1.5 px-4 pb-3">
          {[true, false].map((checked) => (
            <Choice key={String(checked)} label={checked ? 'Checked' : 'Unchecked'} selected={value === checked} onPress={() => onChange(checked)} />
          ))}
        </View>
      )
    case 'select':
    case 'status':
    case 'multi_select':
      return (
        <IdList
          ids={Array.isArray(value) ? (value as string[]) : []}
          onChange={onChange}
          items={options(property).map((option) => ({ id: option.id, name: option.name, pill: <Pill color={option.color} label={option.name} dot={property.type === 'status'} /> }))}
        />
      )
    case 'person':
    case 'created_by':
    case 'last_edited_by':
      return (
        <IdList
          ids={Array.isArray(value) ? (value as string[]) : []}
          onChange={onChange}
          items={[{ id: 'me', name: 'Me' }, ...people.map((person) => ({ id: person.id, name: person.name, leading: <Avatar id={person.id} name={person.name} size={20} /> }))]}
        />
      )
    case 'date':
    case 'created_time':
    case 'last_edited_time':
      return <DateValue value={typeof value === 'string' ? value : null} onChange={onChange} />
  }
}

/** A typed number; the rule keeps the last number while the field holds something else. */
function NumberValue({ value, onChange }: { value: number | null; onChange: (value: number | null) => void }) {
  const [text, setText] = useState(value === null ? '' : String(value))
  return (
    <Input
      label="Value"
      placeholder="Value"
      keyboardType="numbers-and-punctuation"
      value={text}
      autoFocus
      onChangeText={(next) => {
        setText(next)
        const number = Number(next)
        if (next.trim() === '') onChange(null)
        else if (Number.isFinite(number)) onChange(number)
      }}
    />
  )
}

function IdList({ ids, items, onChange }: { ids: string[]; items: { id: string; name: string; leading?: ReactNode; pill?: ReactNode }[]; onChange: (ids: string[]) => void }) {
  const [query, setQuery] = useState('')
  const needle = query.trim().toLowerCase()
  const shown = items.filter((item) => !needle || item.name.toLowerCase().includes(needle))
  return (
    <>
      <SheetSearch value={query} onChange={setQuery} placeholder="Search…" />
      <SheetList>
        {shown.map((item) => {
          const toggle = () => onChange(ids.includes(item.id) ? ids.filter((id) => id !== item.id) : [...ids, item.id])
          return item.pill ? (
            <ChoiceRow key={item.id} label={item.name} checked={ids.includes(item.id)} onPress={toggle}>
              {item.pill}
            </ChoiceRow>
          ) : (
            <SheetRow key={item.id} label={item.name} leading={item.leading} checked={ids.includes(item.id)} onPress={toggle} />
          )
        })}
        {shown.length === 0 ? <SheetNote>Nothing found</SheetNote> : null}
      </SheetList>
    </>
  )
}

/** A relative date (Today, One week ago …) or an exact day from the calendar. */
function DateValue({ value, onChange }: { value: string | null; onChange: (value: string | null) => void }) {
  const relative = value !== null && RELATIVE_DATES.some((date) => date.value === value)
  const exact = !relative && value !== null && /^\d{4}-\d{2}-\d{2}/.test(value) ? value.slice(0, 10) : null
  const [picking, setPicking] = useState(false)
  return (
    <SheetList>
      {RELATIVE_DATES.map((date) => (
        <SheetRow key={date.value} label={date.label} checked={value === date.value} onPress={() => onChange(date.value)} />
      ))}
      <SheetRow
        label={exact ? `Exact date: ${localDate(exact).toLocaleDateString(undefined, { dateStyle: 'medium' })}` : 'Exact date…'}
        icon="calendar"
        checked={exact !== null}
        onPress={() => setPicking((open) => !open)}
      />
      {picking ? (
        <DayPicker
          value={exact ? localDate(exact) : new Date()}
          onPick={(day) => {
            setPicking(false)
            onChange(dayOf(day))
          }}
          onCancel={() => setPicking(false)}
        />
      ) : null}
    </SheetList>
  )
}

/** The sorts of the view, first one first; up and down reorder them. */
export function SortContent({ properties, config, onConfig }: ConfigProps) {
  const { colors } = useTheme()
  const [adding, setAdding] = useState(false)
  const sorts = config.sorts
  const byId = new Map(properties.map((property) => [property.id, property]))
  const setSorts = (next: DatabaseViewSort[]) => onConfig({ ...config, sorts: next })
  const unsorted = properties.filter((property) => !sorts.some((sort) => sort.property_id === property.id))
  const move = (from: number, to: number) => {
    const next = [...sorts]
    const [moved] = next.splice(from, 1)
    next.splice(to, 0, moved!)
    setSorts(next)
  }
  if (adding) {
    return (
      <PropertyChoices
        properties={unsorted}
        onPick={(property) => {
          setAdding(false)
          setSorts([...sorts, { property_id: property.id, direction: 'asc' }])
        }}
      />
    )
  }
  return (
    <ScrollView keyboardShouldPersistTaps="handled" className="shrink">
      {sorts.length === 0 ? <SheetNote>No sorts. Rows keep their manual order.</SheetNote> : null}
      {sorts.map((sort, index) => {
        const property = byId.get(sort.property_id)
        if (!property) return null
        return (
          <View key={sort.property_id} className="min-h-12 flex-row items-center gap-2 pl-4 pr-2">
            <Icon name={PROPERTY_ICONS[property.type]} size={18} color={colors.mutedForeground} />
            <Text numberOfLines={1} className="font-noto-medium text-[14px] flex-1">
              {property.name}
            </Text>
            <Choice
              label={sort.direction === 'asc' ? 'Ascending' : 'Descending'}
              selected={false}
              onPress={() => setSorts(sorts.map((other, at) => (at === index ? { ...other, direction: other.direction === 'asc' ? 'desc' : 'asc' } : other)))}
            />
            <IconButton icon="arrowUp" label={`Move ${property.name} sort up`} color={colors.mutedForeground} disabled={index === 0} onPress={() => move(index, index - 1)} />
            <IconButton icon="arrowDown" label={`Move ${property.name} sort down`} color={colors.mutedForeground} disabled={index === sorts.length - 1} onPress={() => move(index, index + 1)} />
            <IconButton icon="close" label={`Remove ${property.name} sort`} color={colors.mutedForeground} onPress={() => setSorts(sorts.filter((_, at) => at !== index))} />
          </View>
        )
      })}
      {sorts.length < MAX_SORTS && unsorted.length > 0 ? <SheetRow label="Add sort" icon="plus" onPress={() => setAdding(true)} /> : null}
      {sorts.length > 0 ? <SheetRow label="Delete all sorts" icon="trash" onPress={() => setSorts([])} /> : null}
    </ScrollView>
  )
}
