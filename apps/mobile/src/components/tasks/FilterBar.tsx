import { useState, type ReactNode } from 'react'
import { Pressable, View } from 'react-native'

import {
  appendCondition,
  chosenValues,
  countConditions,
  defaultValue,
  ESTIMATE_PRESENCE,
  FIELD_META,
  fieldsFor,
  isCompleteCondition,
  isFlatFilter,
  listValue,
  MAX_FILTER_CONDITIONS,
  operatorLabel,
  operatorNeedsValue,
  RANGE_DATE_PRESETS,
  selectedOptions,
  SINGLE_DATE_PRESETS,
  valueOptions,
  valueSummary,
  withOperator,
  type Condition,
  type FilterField,
  type FilterGroup,
  type FilterOptions,
  type FilterOperator,
  type Glyph,
} from '../../data/tasks-filter'
import { useTheme } from '../../theme'
import { Avatar, Icon, PickerSheet, PriorityIcon, StatusIcon, Text, type PickerOption } from '../../ui'

/**
 * What the one sheet shows: the field list of a new filter, the values of a condition (`index` null: a new one, not in
 * the tree until it is complete), or the operators of a chip.
 */
type Editor =
  | { kind: 'fields' }
  | { kind: 'values'; index: number | null; draft: Condition; base: FilterGroup | null }
  | { kind: 'operator'; index: number }

const sameValue = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

/** The value's own mark: status ring, priority bars, label/project dot, avatar, or a plain icon. */
export function ValueGlyph({ glyph, size = 14 }: { glyph: Glyph; size?: number }) {
  const { colors } = useTheme()
  switch (glyph.kind) {
    case 'status':
      return <StatusIcon status={glyph.status} size={size} />
    case 'priority':
      return <PriorityIcon priority={glyph.priority} size={size} />
    case 'dot':
      return <View className="size-2 rounded-full" style={{ backgroundColor: glyph.color }} />
    case 'member':
      return <Avatar id={glyph.id} name={glyph.name} size={size + 2} />
    case 'icon':
      return <Icon name={glyph.icon} size={14} color={colors.mutedForeground} />
  }
}

/**
 * The web's chip row under the task header (`FilterBar`): the page's preset as a locked chip, one chip per condition,
 * `+` to add one, and Clear all. `adding` opens the field list from outside (the header's Filter button).
 */
export function FilterBar({
  filter,
  options,
  onChange,
  presetLabel,
  adding,
  onAddingChange,
}: {
  filter: FilterGroup
  options: FilterOptions
  onChange: (filter: FilterGroup) => void
  presetLabel: string | null
  adding: boolean
  onAddingChange: (adding: boolean) => void
}) {
  const { colors } = useTheme()
  const [editor, setEditor] = useState<Editor | null>(null)
  // the header's Filter button opens the field list
  if (adding && editor === null) setEditor({ kind: 'fields' })
  const count = countConditions(filter)
  const full = count >= MAX_FILTER_CONDITIONS
  const flat = isFlatFilter(filter)
  const conditions = flat ? (filter.children as Condition[]) : []

  const replaceAt = (index: number, next: Condition) => onChange({ ...filter, children: filter.children.map((child, i) => (i === index ? next : child)) })
  const removeAt = (index: number) => onChange({ ...filter, children: filter.children.filter((_, i) => i !== index) })

  const close = () => {
    // emptying the values of a chip removes it; abandoning a new filter leaves the tree as it was
    if (editor?.kind === 'values' && editor.index !== null && !isCompleteCondition(editor.draft)) {
      const current = conditions[editor.index]
      if (current && current.operator === editor.draft.operator) removeAt(editor.index)
    }
    setEditor(null)
    onAddingChange(false)
  }

  /** Every value edit: a new condition joins the tree once complete, a chip changes once complete. */
  const edit = (next: Condition, done = false) => {
    if (editor?.kind !== 'values') return
    if (editor.index === null) {
      const base = editor.base ?? filter
      if (isCompleteCondition(next)) {
        onChange(appendCondition(base, next))
        setEditor({ ...editor, draft: next, base })
      } else {
        if (editor.base) onChange(editor.base)
        setEditor({ ...editor, draft: next, base: null })
      }
    } else {
      if (isCompleteCondition(next)) replaceAt(editor.index, next)
      setEditor({ ...editor, draft: next })
    }
    if (done) {
      setEditor(null)
      onAddingChange(false)
    }
  }

  const pickField = (field: FilterField) => {
    const operator = FIELD_META[field].defaultOperator
    setEditor({ kind: 'values', index: null, draft: { field, operator, value: defaultValue(field, operator) }, base: null })
  }

  const pickOperator = (index: number, operator: FilterOperator) => {
    const condition = conditions[index]
    if (!condition) return
    const next = withOperator(condition, operator)
    if (isCompleteCondition(next)) {
      replaceAt(index, next)
      setEditor(null)
    } else setEditor({ kind: 'values', index, draft: next, base: null })
  }

  let sheet: { title: string; options: PickerOption[]; onSelect: (key: string) => void } | null = null
  if (editor?.kind === 'fields') {
    sheet = {
      title: 'Filter by',
      options: fieldsFor(options.projectId).map((field) => ({ key: field, label: FIELD_META[field].label, leading: <Icon name={FIELD_META[field].icon} size={14} color={colors.mutedForeground} /> })),
      onSelect: (key) => pickField(key as FilterField),
    }
  } else if (editor?.kind === 'operator') {
    const condition = conditions[editor.index]
    if (condition) {
      const meta = FIELD_META[condition.field]
      const values = meta.kind === 'list' || meta.kind === 'task' ? listValue(condition).length : 1
      sheet = {
        title: meta.label,
        options: meta.operators.map((operator) => ({ key: operator, label: operatorLabel(operator, values), selected: operator === condition.operator })),
        onSelect: (key) => pickOperator(editor.index, key as FilterOperator),
      }
    }
  } else if (editor?.kind === 'values') {
    const { draft } = editor
    const meta = FIELD_META[draft.field]
    if (draft.field === 'estimate') {
      sheet = {
        title: meta.label,
        options: ESTIMATE_PRESENCE.map((choice) => ({ key: choice.operator, label: choice.label, selected: editor.index !== null && choice.operator === draft.operator })),
        onSelect: (key) => edit({ field: draft.field, operator: key as FilterOperator }, true),
      }
    } else if (meta.kind === 'date') {
      const presets = draft.operator === 'between' ? RANGE_DATE_PRESETS : SINGLE_DATE_PRESETS
      sheet = {
        title: meta.label,
        options: [
          ...presets.map((preset) => ({ key: preset.label, label: preset.label, selected: sameValue(preset.value, draft.value) })),
          ...(draft.field === 'due_date' ? [{ key: 'is_empty', label: 'No due date' }, { key: 'is_not_empty', label: 'Has a due date' }] : []),
        ],
        onSelect: (key) => {
          if (key === 'is_empty' || key === 'is_not_empty') return edit({ field: draft.field, operator: key }, true)
          const preset = presets.find((item) => item.label === key)
          if (preset) edit({ field: draft.field, operator: draft.operator, value: preset.value }, true)
        },
      }
    } else if (meta.kind === 'list') {
      const selected = chosenValues(draft, options)
      sheet = {
        title: meta.label,
        options: valueOptions(draft.field, options, colors.mutedForeground).map((option) => ({
          key: option.value,
          label: option.label,
          selected: selected.includes(option.value),
          leading: (
            <View className="size-4 items-center justify-center">
              <ValueGlyph glyph={option.glyph} />
            </View>
          ),
        })),
        onSelect: (key) => edit({ ...draft, value: selected.includes(key) ? selected.filter((value) => value !== key) : [...selected, key] }),
      }
    }
  }

  const visible = presetLabel !== null || filter.children.length > 0
  return (
    <>
      {visible ? (
        <View accessibilityRole="toolbar" accessibilityLabel="Filters" className="min-h-10 flex-row flex-wrap items-center gap-1.5 border-b border-border bg-background px-2 py-1.5">
          {presetLabel ? (
            <View className="h-7 flex-row items-center gap-1.5 rounded-md border border-primary/25 bg-primary/10 px-2">
              <Icon name="lock" size={12} color={colors.primary} />
              <Text className="font-noto-medium text-[12px] leading-[16px] text-primary">{presetLabel}</Text>
            </View>
          ) : null}
          {flat ? (
            conditions.map((condition, index) => (
              <ConditionChip
                key={index}
                condition={editor?.kind === 'values' && editor.index === index ? editor.draft : condition}
                options={options}
                onOperator={() => setEditor({ kind: 'operator', index })}
                onValues={() => setEditor({ kind: 'values', index, draft: condition, base: null })}
                onRemove={() => removeAt(index)}
              />
            ))
          ) : (
            <View className="h-7 flex-row items-center gap-1.5 rounded-md border border-border bg-background px-2.5 dark:border-input dark:bg-input/30">
              <Icon name="subIssue" size={14} color={colors.mutedForeground} />
              <Text className="text-[12px] leading-[16px]">{`Advanced filter · ${count} ${count === 1 ? 'condition' : 'conditions'}`}</Text>
            </View>
          )}
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Add filter"
            disabled={full}
            onPress={() => setEditor({ kind: 'fields' })}
            hitSlop={8}
            className={`size-6 items-center justify-center rounded-md active:bg-muted ${full ? 'opacity-50' : ''}`}
          >
            <Icon name="plus" size={12} color={colors.mutedForeground} />
          </Pressable>
          {count > 0 ? (
            <View className="ml-auto flex-row items-center">
              <Pressable accessibilityRole="button" onPress={() => onChange({ op: 'and', children: [] })} className="h-7 justify-center rounded-md px-2.5 active:bg-muted">
                <Text className="font-noto-medium text-[12.8px] leading-[18px] text-muted-foreground">Clear all</Text>
              </Pressable>
            </View>
          ) : null}
        </View>
      ) : null}
      <PickerSheet visible={sheet !== null} title={sheet?.title ?? ''} options={sheet?.options ?? []} onSelect={(key) => sheet?.onSelect(key)} onClose={close} />
    </>
  )
}

/** `Field · operator · values ×` as one bordered pill, its parts split by a hairline. */
function ConditionChip({ condition, options, onOperator, onValues, onRemove }: { condition: Condition; options: FilterOptions; onOperator: () => void; onValues: () => void; onRemove: () => void }) {
  const { colors } = useTheme()
  const meta = FIELD_META[condition.field]
  const count = meta.kind === 'list' || meta.kind === 'task' ? listValue(condition).length : 1
  const glyphs = selectedOptions(condition, options, colors.mutedForeground).slice(0, 3)
  const summary = valueSummary(condition, options, colors.mutedForeground)
  const operatorText = operatorLabel(condition.operator, count)
  // the app has no picker for a Parent or Text value: such a chip (made on the web) can only be removed
  const editable = meta.kind === 'list' || meta.kind === 'date'
  return (
    <View className="h-7 max-w-full flex-row items-stretch overflow-hidden rounded-md border border-border bg-background">
      <Segment first>
        <Icon name={meta.icon} size={14} color={colors.mutedForeground} />
        <Text className="text-[12px] leading-[16px] text-muted-foreground">{meta.label}</Text>
      </Segment>
      <Segment onPress={editable && meta.operators.length > 1 ? onOperator : undefined} label={`${meta.label} operator: ${operatorText}`}>
        <Text className="text-[12px] leading-[16px] text-muted-foreground">{operatorText}</Text>
      </Segment>
      {operatorNeedsValue(condition.operator) ? (
        <Segment onPress={editable ? onValues : undefined} label={`${meta.label} values: ${summary || 'none'}`} shrink>
          {glyphs.length > 0 ? (
            <View className="flex-row items-center gap-0.5">
              {glyphs.map((option) => (
                // the web's chip button draws its svg glyphs (status, priority) at 16px; an avatar stays small
                <ValueGlyph key={option.value} glyph={option.glyph} size={option.glyph.kind === 'member' ? 12 : 16} />
              ))}
            </View>
          ) : null}
          <Text numberOfLines={1} className="shrink font-noto-medium text-[12px] leading-[16px]">
            {summary || 'Choose…'}
          </Text>
        </Segment>
      ) : null}
      <Pressable accessibilityRole="button" accessibilityLabel={`Remove ${meta.label.toLowerCase()} filter`} onPress={onRemove} className="w-6 items-center justify-center border-l border-border active:bg-muted">
        <Icon name="x" size={12} color={colors.mutedForeground} />
      </Pressable>
    </View>
  )
}

function Segment({ children, onPress, label, first = false, shrink = false }: { children: ReactNode; onPress?: () => void; label?: string; first?: boolean; shrink?: boolean }) {
  const className = `flex-row items-center gap-1.5 px-2 ${first ? '' : 'border-l border-border'} ${shrink ? 'shrink min-w-0' : ''}`
  if (!onPress) return <View className={className}>{children}</View>
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={label} onPress={onPress} className={`${className} active:bg-muted`}>
      {children}
    </Pressable>
  )
}
