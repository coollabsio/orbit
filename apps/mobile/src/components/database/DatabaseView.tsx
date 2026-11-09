/**
 * A database page at phone size (spec 8): the page header, the view picker, search, Filter and Sort, then the rows of
 * a table view as a list (title and the first 3 visible properties) or the columns of a board view, one screen wide
 * each. View changes save for everyone after 500 ms, or stay on this device for viewers who cannot change views (D1).
 */
import { groupRows, type BoardGroup } from '@orbit/database/board'
import { EMPTY_FILTER, filterRules } from '@orbit/database/filter'
import { ruleSummary, simpleRules } from '@orbit/database/meta'
import { newRowValues } from '@orbit/database/newRow'
import type { Database, DatabaseView as View_, GroupProperty, Property, Row, ViewConfig, ViewContext } from '@orbit/database/types'
import { cellOf } from '@orbit/database/values'
import { applyView, groupable, repairConfig } from '@orbit/database/view'
import { router } from 'expo-router'
import { type ReactElement, useEffect, useState } from 'react'
import { FlatList, Pressable, ScrollView, useWindowDimensions, View } from 'react-native'

import { advancedFilterLabel, filledProperties, groupValues, listProperties, pickView, PROPERTY_ICONS, sortedViews, summaryLines, VIEW_ICONS, viewStorageKey } from '../../data/database'
import { type Notify, useCreateRow, useDatabase, useUpdateValues, useViewContext, useViewDrafts, useWorkspaceReadOnly } from '../../hooks/database'
import { storage } from '../../storage'
import { alpha, useTheme } from '../../theme'
import { Button, ErrorView, Icon, IconButton, LoadingView, PullRefresh, Text } from '../../ui'
import { TextInput } from '../../ui/scaled'
import { Sheet, SheetList, SheetNote, SheetRow, useSheet } from '../tasks/Sheet'
import { CellValue, ChoiceRow, OptionPill, RowTitle } from './cells'
import { FilterContent, SortContent } from './ViewSheets'

/** Cards each board column shows before "Show more", as on the web. */
const PAGE = 50
/** A board column's share of the screen width: the next column peeks in. */
const COLUMN_SHARE = 0.85
const COLUMN_GAP = 10

type DatabaseSheet = { kind: 'views' } | { kind: 'filters' } | { kind: 'sorts' } | { kind: 'move'; rowId: string }

/**
 * `linkId` shows a linked view block's views instead of the database's own (spec 3.7); `initialView` opens one of them
 * (the view a page's card showed) instead of the last one chosen on this device.
 */
export function DatabaseView({ databaseId, linkId = '', initialView = null, header, notify }: {
  databaseId: string
  linkId?: string
  initialView?: string | null
  header: ReactElement
  notify: Notify
}) {
  const query = useDatabase(databaseId, linkId)
  const ctx = useViewContext()
  const readOnly = useWorkspaceReadOnly()
  // The database lock guards its own views only; the server checks a linked view's host page.
  const locked = linkId ? false : (query.data?.database.locked ?? true)
  const canEditSchema = !locked && !readOnly
  const views = useViewDrafts(databaseId, linkId, canEditSchema, notify)
  const createRow = useCreateRow(databaseId, notify, linkId)
  const updateValues = useUpdateValues(databaseId, notify, linkId)
  const sheet = useSheet<DatabaseSheet>()
  const [openRule, setOpenRule] = useState<number | 'add' | null>(null)
  const [search, setSearch] = useState('')
  const [chosen, setChosen] = useState<string | null>(initialView)
  const storageKey = viewStorageKey(databaseId, linkId)

  useEffect(() => {
    void storage.get(storageKey).then((stored) => setChosen((current) => current ?? stored), () => {})
  }, [storageKey])

  const database = query.data
  if (!database) {
    return (
      <ScrollView contentContainerClassName="pb-8">
        {header}
        {query.isError ? <ErrorView error={query.error} onRetry={() => void query.refetch()} /> : <LoadingView />}
      </ScrollView>
    )
  }
  const view = pickView(database.views, chosen)
  if (!view) return null
  const draft = views.drafts[view.id]
  const config = repairConfig({ ...view, config: draft ?? view.config }, database.properties)
  const rows = applyView(database, { ...view, config }, search, ctx)
  const filtered = filterRules(config.filter).length > 0 || search.trim() !== ''
  const onConfig = (next: ViewConfig) => views.change(view.id, next)

  const selectView = (viewId: string) => {
    setChosen(viewId)
    void storage.set(storageKey, viewId).catch(() => {})
    sheet.close()
  }
  /** A new row with the values the filter needs (and a board column's value), opened on its own page. */
  const addRow = (values: Record<string, unknown> = {}) => {
    const fill = newRowValues(config.filter, database.properties, ctx)
    createRow.mutate({ ...(fill.title ? { title: fill.title } : {}), values: { ...fill.values, ...values } }, { onSuccess: (row) => router.push(`/docs/${row.id}`) })
  }
  const clearFilters = () => {
    setSearch('')
    onConfig({ ...config, filter: EMPTY_FILTER })
  }
  const openFilters = (rule: number | 'add' | null) => {
    setOpenRule(rule)
    sheet.open({ kind: 'filters' })
  }

  const toolbar = (
    <Toolbar
      database={database}
      view={view}
      config={config}
      ctx={ctx}
      search={search}
      onSearch={setSearch}
      canAdd={!readOnly}
      adding={createRow.isPending}
      onAdd={() => addRow()}
      onViews={() => sheet.open({ kind: 'views' })}
      onFilters={openFilters}
      onSorts={() => sheet.open({ kind: 'sorts' })}
      localOnly={draft !== undefined && !canEditSchema}
      onReset={() => views.reset(view.id)}
    />
  )
  const empty = <EmptyRows filtered={filtered} onClear={clearFilters} />
  const refresh = <PullRefresh onRefresh={() => query.refetch()} />
  const groupProperty = database.properties.find((property) => property.id === config.group_by)
  const open = sheet.sheet
  const moving = open?.kind === 'move' ? database.rows.find((row) => row.id === open.rowId) : undefined

  return (
    <>
      {view.type === 'board' ? (
        <ScrollView contentContainerClassName="pb-8" keyboardShouldPersistTaps="handled" refreshControl={refresh}>
          {header}
          {toolbar}
          {groupable(groupProperty) ? (
            <Board
              key={view.id}
              database={database}
              config={config}
              rows={rows}
              property={groupProperty}
              ctx={ctx}
              canEdit={!readOnly}
              onAdd={(group) => addRow(groupValues(groupProperty, group.id))}
              onMove={(row) => sheet.open({ kind: 'move', rowId: row.id })}
            />
          ) : (
            <View className="items-center gap-1 py-10 px-6">
              <Text className="text-[14px] text-muted-foreground">Choose a property to group by</Text>
              <Text className="text-[12px] text-muted-foreground text-center">A board groups its cards by a Status or Select property. Edit on the web.</Text>
            </View>
          )}
          {rows.length === 0 && filtered ? empty : null}
        </ScrollView>
      ) : (
        <RowList database={database} config={config} rows={rows} ctx={ctx} header={<>{header}{toolbar}</>} empty={empty} refresh={refresh} />
      )}
      <Sheet visible={sheet.visible} title={sheetTitle(sheet.sheet, moving)} onClose={sheet.close}>
        {sheet.sheet?.kind === 'views' ? (
          <SheetList>
            {sortedViews(database.views).map((candidate) => (
              <SheetRow key={candidate.id} label={candidate.name} icon={VIEW_ICONS[candidate.type]} checked={candidate.id === view.id} onPress={() => selectView(candidate.id)} />
            ))}
            <SheetNote>Add, rename and set up views on the web.</SheetNote>
          </SheetList>
        ) : sheet.sheet?.kind === 'filters' ? (
          <FilterContent properties={database.properties} config={config} onConfig={onConfig} ctx={ctx} openRule={openRule} onOpenRule={setOpenRule} />
        ) : sheet.sheet?.kind === 'sorts' ? (
          <SortContent properties={database.properties} config={config} onConfig={onConfig} />
        ) : moving && groupable(groupProperty) ? (
          <MoveChoices
            row={moving}
            property={groupProperty}
            config={config}
            onPick={(group) => {
              sheet.close()
              updateValues.mutate({ rowId: moving.id, values: groupValues(groupProperty, group.id) })
            }}
          />
        ) : null}
      </Sheet>
    </>
  )
}

function sheetTitle(sheet: DatabaseSheet | null, moving: Row | undefined): string {
  switch (sheet?.kind) {
    case 'views':
      return 'Views'
    case 'filters':
      return 'Filter'
    case 'sorts':
      return 'Sort'
    case 'move':
      return `Move “${moving?.title || 'Untitled'}” to…`
    case undefined:
      return ''
  }
}

function Toolbar(props: {
  database: Database
  view: View_
  config: ViewConfig
  ctx: ViewContext
  search: string
  onSearch: (text: string) => void
  canAdd: boolean
  adding: boolean
  onAdd: () => void
  onViews: () => void
  onFilters: (rule: number | 'add' | null) => void
  onSorts: () => void
  localOnly: boolean
  onReset: () => void
}) {
  const { database, view, config, ctx, search, onSearch } = props
  const { colors, scheme } = useTheme()
  const muted = alpha(colors.mutedForeground, 0.7)
  const byId = new Map(database.properties.map((property) => [property.id, property]))
  const count = filterRules(config.filter).length
  const rules = simpleRules(config.filter)
  return (
    <View className="px-3 gap-2 pb-2">
      <View className="flex-row items-center gap-1 border-b border-border pb-1.5">
        <Pressable accessibilityRole="button" accessibilityLabel={`View: ${view.name}`} onPress={props.onViews} className="h-8 flex-row items-center gap-1.5 px-2 rounded-md active:bg-muted shrink">
          <Icon name={VIEW_ICONS[view.type]} size={15} color={colors.foreground} />
          <Text numberOfLines={1} className="font-noto-medium text-[14px] shrink">
            {view.name}
          </Text>
          <Icon name="chevronDown" size={12} color={colors.mutedForeground} />
        </Pressable>
        <View className="flex-1" />
        <IconButton icon="filter" label={count > 0 ? `Filter, ${count}` : 'Filter'} color={count > 0 ? colors.primary : muted} onPress={() => props.onFilters(null)} />
        <IconButton icon="sort" label={config.sorts.length > 0 ? `Sort, ${config.sorts.length}` : 'Sort'} color={config.sorts.length > 0 ? colors.primary : muted} onPress={props.onSorts} />
        {props.canAdd ? <IconButton icon="plus" label="New row" size={28} variant="primary" disabled={props.adding} onPress={props.onAdd} /> : null}
      </View>
      <View className="h-9 flex-row items-center gap-2 px-2.5 rounded-lg border border-input">
        <Icon name="search" size={15} color={colors.mutedForeground} />
        <TextInput
          value={search}
          onChangeText={onSearch}
          placeholder="Search"
          accessibilityLabel="Search rows"
          placeholderTextColor={colors.mutedForeground}
          selectionColor={colors.primary}
          keyboardAppearance={scheme}
          autoCorrect={false}
          autoCapitalize="none"
          returnKeyType="search"
          className="flex-1 font-noto text-[16px] leading-[22px] p-0 text-foreground"
        />
        {search ? <IconButton icon="close" label="Clear search" size={22} iconSize={13} color={muted} onPress={() => onSearch('')} /> : null}
      </View>
      {rules === null ? (
        <View accessibilityLabel="Filters" className="flex-row flex-wrap gap-1.5">
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={advancedFilterLabel(count)}
            onPress={() => props.onFilters(null)}
            className="h-7 max-w-full flex-row items-center gap-1.5 px-2 rounded-md border border-border bg-background active:bg-muted"
          >
            <Icon name="filter" size={13} color={colors.primary} />
            <Text numberOfLines={1} className="text-[12px] shrink text-primary">
              {advancedFilterLabel(count)}
            </Text>
          </Pressable>
        </View>
      ) : rules.length > 0 ? (
        <View accessibilityLabel="Filters" className="flex-row flex-wrap gap-1.5">
          {rules.map((rule, index) => {
            const property = byId.get(rule.property_id)
            if (!property) return null
            const summary = ruleSummary(rule, property, ctx)
            return (
              <Pressable
                key={`${index}:${rule.property_id}`}
                accessibilityRole="button"
                accessibilityLabel={`${property.name} filter: ${summary}`}
                onPress={() => props.onFilters(index)}
                className="h-7 max-w-full flex-row items-center gap-1.5 px-2 rounded-md border border-border bg-background active:bg-muted"
              >
                <Icon name={PROPERTY_ICONS[property.type]} size={13} color={summary === 'All' ? colors.foreground : colors.primary} />
                <Text numberOfLines={1} className={`text-[12px] shrink ${summary === 'All' ? 'text-foreground' : 'text-primary'}`}>
                  <Text className="font-noto-medium text-[12px]">{property.name}:</Text> {summary}
                </Text>
              </Pressable>
            )
          })}
        </View>
      ) : null}
      {props.localOnly ? (
        <View accessibilityRole="text" className="flex-row items-center gap-1.5">
          <Text className="text-[12px] text-muted-foreground">Changes are only visible to you ·</Text>
          <Text accessibilityRole="button" onPress={props.onReset} className="font-noto-medium text-[12px] text-foreground underline">
            Reset
          </Text>
        </View>
      ) : null}
    </View>
  )
}

function EmptyRows({ filtered, onClear }: { filtered: boolean; onClear: () => void }) {
  return (
    <View className="items-center gap-2 py-10">
      <Text className="text-[14px] text-muted-foreground">{filtered ? 'No results' : 'No rows yet'}</Text>
      {filtered ? <Button label="Clear filters" variant="outline" size="sm" onPress={onClear} /> : null}
    </View>
  )
}

/** A table view as a list: each row is its title and the first 3 visible properties that have a value. */
function RowList({ database, config, rows, ctx, header, empty, refresh }: {
  database: Database
  config: ViewConfig
  rows: Row[]
  ctx: ViewContext
  header: ReactElement
  empty: ReactElement
  refresh: ReactElement
}) {
  const shown = listProperties(database.properties, config)
  const summary = summaryLines(database.properties, config, rows, ctx)
  return (
    <FlatList
      data={rows}
      keyExtractor={(row) => row.id}
      ListHeaderComponent={header}
      ListEmptyComponent={empty}
      ListFooterComponent={summary.length > 0 ? <Summary lines={summary} /> : undefined}
      keyboardShouldPersistTaps="handled"
      contentContainerClassName="pb-8"
      refreshControl={refresh}
      renderItem={({ item }) => <ListRowItem row={item} properties={filledProperties(item, shown)} ctx={ctx} />}
    />
  )
}

/** The web footer's calculations, read only (spec P2 4.6). */
function Summary({ lines }: { lines: { id: string; text: string }[] }) {
  return (
    <View accessibilityLabel="Summary" className="mx-3 pt-4 px-1.5 gap-1">
      <Text className="font-noto-medium text-[12px] text-muted-foreground">Summary</Text>
      {lines.map((line) => (
        <Text key={line.id} className="text-[13px] text-foreground">
          {line.text}
        </Text>
      ))}
      <Text className="text-[12px] text-muted-foreground">To change a calculation, edit on the web.</Text>
    </View>
  )
}

function ListRowItem({ row, properties, ctx }: { row: Row; properties: Property[]; ctx: ViewContext }) {
  return (
    <Pressable accessibilityRole="link" accessibilityLabel={row.title || 'Untitled'} onPress={() => router.push(`/docs/${row.id}`)} className="mx-3 py-2.5 px-1.5 gap-1.5 border-b border-border active:bg-muted">
      <RowTitle row={row} />
      {properties.length > 0 ? (
        <View className="flex-row flex-wrap items-center gap-1.5">
          {properties.map((property) => (
            <CellValue key={property.id} property={property} row={row} ctx={ctx} chip />
          ))}
        </View>
      ) : null}
    </Pressable>
  )
}

/** The board columns side by side, one screen wide each and snapping; hidden groups stay out. */
function Board({ database, config, rows, property, ctx, canEdit, onAdd, onMove }: {
  database: Database
  config: ViewConfig
  rows: Row[]
  property: GroupProperty
  ctx: ViewContext
  canEdit: boolean
  onAdd: (group: BoardGroup) => void
  onMove: (row: Row) => void
}) {
  const { width } = useWindowDimensions()
  const columnWidth = Math.round(width * COLUMN_SHARE)
  const groups = groupRows(rows, property, config)
  const visible = groups.filter((group) => !group.hidden)
  const hidden = groups.length - visible.length
  const cardProperties = config.card_properties.flatMap((id) => database.properties.find((candidate) => candidate.id === id) ?? [])
  return (
    <>
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        snapToInterval={columnWidth + COLUMN_GAP}
        decelerationRate="fast"
        contentContainerStyle={{ paddingHorizontal: 12, gap: COLUMN_GAP }}
      >
        {visible.map((group) => (
          <Column key={group.id ?? 'none'} group={group} property={property} cardProperties={cardProperties} ctx={ctx} width={columnWidth} canEdit={canEdit} onAdd={() => onAdd(group)} onMove={onMove} />
        ))}
      </ScrollView>
      {hidden > 0 ? <Text className="text-[12px] px-4 pt-3 text-muted-foreground">{hidden === 1 ? '1 hidden group' : `${hidden} hidden groups`}. Show them on the web.</Text> : null}
    </>
  )
}

function Column({ group, property, cardProperties, ctx, width, canEdit, onAdd, onMove }: {
  group: BoardGroup
  property: GroupProperty
  cardProperties: Property[]
  ctx: ViewContext
  width: number
  canEdit: boolean
  onAdd: () => void
  onMove: (row: Row) => void
}) {
  const { colors } = useTheme()
  const [shown, setShown] = useState(PAGE)
  const name = group.option?.name ?? `No ${property.name}`
  return (
    <View accessibilityLabel={name} className="rounded-lg bg-muted/40 p-2 gap-2" style={{ width }}>
      <View className="h-7 flex-row items-center gap-2 px-1">
        {group.id === null ? <Text className="text-[13px] text-muted-foreground">{name}</Text> : <OptionPill property={property} id={group.id} />}
        <Text className="text-[12px] text-muted-foreground">{group.rows.length}</Text>
        <View className="flex-1" />
        {canEdit ? <IconButton icon="plus" label={`Add a card to ${name}`} size={24} iconSize={14} color={colors.mutedForeground} onPress={onAdd} /> : null}
      </View>
      {group.rows.slice(0, shown).map((row) => (
        <Card key={row.id} row={row} properties={cardProperties} ctx={ctx} onMove={canEdit ? () => onMove(row) : undefined} />
      ))}
      {group.rows.length > shown ? <Button label={`Show ${Math.min(PAGE, group.rows.length - shown)} more`} variant="ghost" size="sm" muted onPress={() => setShown((count) => count + PAGE)} /> : null}
    </View>
  )
}

/** A card: the title, then each card property that has a value. A long press offers "Move to…". */
function Card({ row, properties, ctx, onMove }: { row: Row; properties: Property[]; ctx: ViewContext; onMove?: () => void }) {
  const filled = properties.filter((property) => cellOf(row, property) !== null)
  return (
    <Pressable
      accessibilityRole="link"
      accessibilityLabel={row.title || 'Untitled'}
      accessibilityHint={onMove ? 'Long press to move it to another group' : undefined}
      onPress={() => router.push(`/docs/${row.id}`)}
      onLongPress={onMove}
      className="gap-1.5 rounded-md border border-border bg-card px-2.5 py-2 active:bg-muted"
    >
      <RowTitle row={row} />
      {filled.map((property) => (
        <CellValue key={property.id} property={property} row={row} ctx={ctx} chip />
      ))}
    </Pressable>
  )
}

/** The board's visible groups but the card's own: a pick sets the group property. */
function MoveChoices({ row, property, config, onPick }: { row: Row; property: GroupProperty; config: ViewConfig; onPick: (group: BoardGroup) => void }) {
  const current = cellOf(row, property)
  const currentId = current?.kind === 'choice' ? current.id : null
  const groups = groupRows([], property, config).filter((group) => !group.hidden)
  return (
    <SheetList>
      {groups.map((group) => (
        <ChoiceRow key={group.id ?? 'none'} label={group.option?.name ?? `No ${property.name}`} checked={group.id === currentId} onPress={() => (group.id === currentId ? undefined : onPick(group))}>
          {group.id === null ? <Text className="text-[14px] text-muted-foreground">No {property.name}</Text> : <OptionPill property={property} id={group.id} />}
        </ChoiceRow>
      ))}
    </SheetList>
  )
}
