/**
 * A database page at phone size (spec 8): the page header, the view picker, search, Filter and Sort, then the rows of
 * a table view as a list (title and the first 3 visible properties), a list view (all its visible properties), a
 * calendar view as a month grid with a day list, a gallery as a grid of cards, a timeline as a list by month, or the columns of a board view, one screen wide
 * each (with sub-groups, a lane picker shows one lane at a time). View changes save for everyone after 500 ms, or stay on this device for viewers who cannot change views (D1).
 */
import { groupName, groupRows, groupValues, laneRows, moveValues, type BoardGroup, type BoardLane } from '@orbit/database/board'
import { EMPTY_FILTER, filterRules } from '@orbit/database/filter'
import { ruleSummary, simpleRules } from '@orbit/database/meta'
import { newRowValues } from '@orbit/database/newRow'
import { defaultTemplate, templateFill, templatesOf } from '@orbit/database/templates'
import type { Database, DatabaseView as View_, GroupProperty, LaneProperty, Property, Row, ViewConfig, ViewContext } from '@orbit/database/types'
import { cellOf } from '@orbit/database/values'
import { applyView, groupable, laneable, repairConfig } from '@orbit/database/view'
import { withRelatedPages } from '@orbit/database/relations'
import { router } from 'expo-router'
import { type ReactElement, useEffect, useState } from 'react'
import { FlatList, Pressable, ScrollView, useWindowDimensions, View } from 'react-native'

import { advancedFilterLabel, filledProperties, listProperties, pickView, PROPERTY_ICONS, sortedViews, summaryLines, VIEW_ICONS, viewStorageKey } from '../../data/database'
import { type Notify, useCreateRow, useDatabase, useUpdateValues, useViewContext, useViewDrafts, useWorkspaceReadOnly } from '../../hooks/database'
import { storage } from '../../storage'
import { alpha, useTheme } from '../../theme'
import { Avatar, Button, ErrorView, Icon, IconButton, LoadingView, PullRefresh, Text } from '../../ui'
import { TextInput } from '../../ui/scaled'
import { Sheet, SheetList, SheetNote, SheetRow, useSheet } from '../tasks/Sheet'
import { ButtonPressContext } from './ButtonCell'
import { Calendar } from './Calendar'
import { Gallery } from './Gallery'
import { TimelineList } from './TimelineList'
import { FormFill, FormViewPicker } from './FormFill'
import { CellValue, ChoiceRow, OptionPill, RowTitle } from './cells'
import { shareViewCsv } from './csvShare'
import { FilterContent, SortContent } from './ViewSheets'

/** Cards each board column shows before "Show more", as on the web. */
const PAGE = 50
/** A board column's share of the screen width: the next column peeks in. */
const COLUMN_SHARE = 0.85
const COLUMN_GAP = 10

/** Where a card sits: its column, and its lane (`null` on a board without lanes). */
type Cell = { group: string | null; lane: string | null }
type DatabaseSheet = { kind: 'views' } | { kind: 'filters' } | { kind: 'sorts' } | { kind: 'more' } | { kind: 'move'; rowId: string; from: Cell } | { kind: 'templates' }

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
  // Relation values, filters and the CSV export show the related pages' titles.
  const ctx = withRelatedPages(useViewContext(), query.data)
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
  const templates = templatesOf(database)
  /**
   * A new row with the values the filter needs (and a board column's value), opened on its own page. It fills from
   * `templateId` (`null`: empty), by default the view's default template (spec P2 7.5).
   */
  const addRow = (values: Record<string, unknown> = {}, templateId: string | null = defaultTemplate(config, templates)?.id ?? null) => {
    const fill = newRowValues(config.filter, database.properties, ctx)
    createRow.mutate(
      { ...(fill.title ? { title: fill.title } : {}), values: { ...fill.values, ...values }, ...templateFill(templateId, ctx) },
      { onSuccess: (row) => router.push(`/docs/${row.id}`) },
    )
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
      onAddLongPress={() => sheet.open({ kind: 'templates' })}
      onViews={() => sheet.open({ kind: 'views' })}
      onFilters={openFilters}
      onSorts={() => sheet.open({ kind: 'sorts' })}
      onMore={() => sheet.open({ kind: 'more' })}
      localOnly={draft !== undefined && !canEditSchema}
      onReset={() => views.reset(view.id)}
    />
  )
  const empty = <EmptyRows filtered={filtered} onClear={clearFilters} />
  const refresh = <PullRefresh onRefresh={() => query.refetch()} />
  const groupProperty = database.properties.find((property) => property.id === config.group_by)
  const lanePick = database.properties.find((property) => property.id === config.sub_group_by)
  const laneProperty = laneable(lanePick) ? lanePick : undefined
  const cellValues = (property: GroupProperty, cell: Cell) => ({ ...groupValues(property, cell.group), ...(laneProperty ? groupValues(laneProperty, cell.lane) : {}) })
  const open = sheet.sheet
  const moving = open?.kind === 'move' ? database.rows.find((row) => row.id === open.rowId) : undefined

  return (
    <ButtonPressContext.Provider value={{ databaseId, linkId, notify }}>
      {view.type === 'form' ? (
        <FormFill
          key={view.id}
          database={database}
          viewId={view.id}
          config={config}
          ctx={ctx}
          header={<>{header}<FormViewPicker view={view} onPress={() => sheet.open({ kind: 'views' })} /></>}
          canSubmit={!readOnly}
          editOnWeb={canEditSchema}
        />
      ) : view.type === 'board' ? (
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
              laneProperty={laneProperty}
              ctx={ctx}
              canEdit={!readOnly}
              onAdd={(cell) => addRow(cellValues(groupProperty, cell))}
              onMove={(row, from) => sheet.open({ kind: 'move', rowId: row.id, from })}
            />
          ) : (
            <View className="items-center gap-1 py-10 px-6">
              <Text className="text-[14px] text-muted-foreground">Choose a property to group by</Text>
              <Text className="text-[12px] text-muted-foreground text-center">A board groups its cards by a Status, Select, Multi-select or Person property. Edit on the web.</Text>
            </View>
          )}
          {rows.length === 0 && filtered ? empty : null}
        </ScrollView>
      ) : view.type === 'gallery' ? (
        <Gallery key={view.id} database={database} config={config} rows={rows} ctx={ctx} header={<>{header}{toolbar}</>} empty={empty} refresh={refresh} />
      ) : view.type === 'timeline' ? (
        <TimelineList key={view.id} database={database} config={config} rows={rows} ctx={ctx} header={<>{header}{toolbar}</>} empty={empty} refresh={refresh} />
      ) : view.type === 'calendar' ? (
        <Calendar key={view.id} database={database} config={config} rows={rows} ctx={ctx} header={<>{header}{toolbar}</>} refresh={refresh} canAdd={!readOnly} onAdd={(values) => addRow(values)} />
      ) : (
        <RowList database={database} config={config} rows={rows} ctx={ctx} header={<>{header}{toolbar}</>} empty={empty} refresh={refresh} allProperties={view.type === 'list'} />
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
        ) : sheet.sheet?.kind === 'templates' ? (
          <SheetList>
            {templates.map((template) => (
              <SheetRow
                key={template.id}
                label={template.name}
                subtitle={template.id === config.default_template_id ? 'Default for this view' : undefined}
                icon="docs"
                onPress={() => {
                  sheet.close()
                  addRow({}, template.id)
                }}
              />
            ))}
            <SheetRow
              label="Empty"
              icon="plus"
              onPress={() => {
                sheet.close()
                addRow({}, null)
              }}
            />
            <SheetNote>Create and edit templates on the web.</SheetNote>
          </SheetList>
        ) : sheet.sheet?.kind === 'sorts' ? (
          <SortContent properties={database.properties} config={config} onConfig={onConfig} />
        ) : sheet.sheet?.kind === 'more' ? (
          <SheetList>
            <SheetRow
              label="Export view as CSV"
              icon="documentDownload"
              onPress={() => {
                sheet.close()
                shareViewCsv(database, { ...view, config }, rows, ctx).catch(() => notify('Could not export the view.'))
              }}
            />
            <SheetNote>Import a CSV or merge one into this database on the web.</SheetNote>
          </SheetList>
        ) : open?.kind === 'move' && moving && groupable(groupProperty) ? (
          <MoveChoices
            key={moving.id}
            rows={rows}
            property={groupProperty}
            laneProperty={laneProperty}
            config={config}
            ctx={ctx}
            from={open.from}
            onPick={(to) => {
              sheet.close()
              const values = { ...moveValues(moving, groupProperty, open.from.group, to.group), ...(laneProperty ? moveValues(moving, laneProperty, open.from.lane, to.lane) : {}) }
              if (Object.keys(values).length > 0) updateValues.mutate({ rowId: moving.id, values })
            }}
          />
        ) : null}
      </Sheet>
    </ButtonPressContext.Provider>
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
    case 'more':
      return 'View options'
    case 'templates':
      return 'New row from'
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
  /** Lists the templates (a long press on "+"). */
  onAddLongPress: () => void
  onViews: () => void
  onFilters: (rule: number | 'add' | null) => void
  onSorts: () => void
  onMore: () => void
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
        <IconButton icon="more" label="View options" color={muted} onPress={props.onMore} />
        {props.canAdd ? <IconButton icon="plus" label="New row" size={28} variant="primary" disabled={props.adding} onPress={props.onAdd} onLongPress={props.onAddLongPress} /> : null}
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

/** A table or list view as a list: each row is its title and the visible properties that have a value (a table: the first 3). */
function RowList({ database, config, rows, ctx, header, empty, refresh, allProperties }: {
  database: Database
  config: ViewConfig
  rows: Row[]
  ctx: ViewContext
  header: ReactElement
  empty: ReactElement
  refresh: ReactElement
  allProperties: boolean
}) {
  const shown = listProperties(database.properties, config, allProperties ? Infinity : undefined)
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

/**
 * The board columns side by side, one screen wide each and snapping; hidden groups stay out. With sub-groups, a lane
 * picker at the top shows the columns of one lane at a time (spec P2 8.6).
 */
function Board({ database, config, rows, property, laneProperty, ctx, canEdit, onAdd, onMove }: {
  database: Database
  config: ViewConfig
  rows: Row[]
  property: GroupProperty
  laneProperty: LaneProperty | undefined
  ctx: ViewContext
  canEdit: boolean
  onAdd: (cell: Cell) => void
  onMove: (row: Row, from: Cell) => void
}) {
  const { width } = useWindowDimensions()
  const [picked, setPicked] = useState<{ id: string | null } | null>(null)
  const columnWidth = Math.round(width * COLUMN_SHARE)
  const lanes = laneProperty ? laneRows(rows, property, laneProperty, config, ctx.members).filter((lane) => !lane.hidden) : []
  const lane = laneProperty ? (lanes.find((candidate) => picked !== null && candidate.id === picked.id) ?? lanes[0]) : undefined
  const groups = lane ? lane.columns : groupRows(rows, property, config, ctx.members)
  const visible = groups.filter((group) => !group.hidden)
  const hidden = groups.length - visible.length
  const laneId = lane?.id ?? null
  const cardProperties = config.card_properties.flatMap((id) => database.properties.find((candidate) => candidate.id === id) ?? [])
  return (
    <>
      {laneProperty && lanes.length > 0 ? <LanePicker lanes={lanes} property={laneProperty} ctx={ctx} selected={laneId} onSelect={(id) => setPicked({ id })} /> : null}
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        snapToInterval={columnWidth + COLUMN_GAP}
        decelerationRate="fast"
        contentContainerStyle={{ paddingHorizontal: 12, gap: COLUMN_GAP }}
      >
        {visible.map((group) => (
          <Column
            key={JSON.stringify([group.id, laneId])}
            group={group}
            property={property}
            cardProperties={cardProperties}
            ctx={ctx}
            width={columnWidth}
            canEdit={canEdit}
            onAdd={() => onAdd({ group: group.id, lane: laneId })}
            onMove={(row) => onMove(row, { group: group.id, lane: laneId })}
          />
        ))}
      </ScrollView>
      {hidden > 0 ? <Text className="text-[12px] px-4 pt-3 text-muted-foreground">{hidden === 1 ? '1 hidden group' : `${hidden} hidden groups`}. Show them on the web.</Text> : null}
    </>
  )
}

/** The lanes as a row of chips, each with its card count; the chosen one is the lane the columns show. */
function LanePicker({ lanes, property, ctx, selected, onSelect }: {
  lanes: BoardLane[]
  property: LaneProperty
  ctx: ViewContext
  selected: string | null
  onSelect: (id: string | null) => void
}) {
  return (
    <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={{ paddingHorizontal: 12, paddingBottom: 10, gap: 6 }}>
      {lanes.map((lane) => {
        const active = lane.id === selected
        return (
          <Pressable
            key={lane.id ?? 'none'}
            accessibilityRole="button"
            accessibilityLabel={`${groupName(property, lane, ctx.members)}, ${lane.rows.length}`}
            accessibilityState={{ selected: active }}
            onPress={() => onSelect(lane.id)}
            className={`h-8 flex-row items-center gap-1.5 rounded-full border px-3 ${active ? 'border-primary bg-primary/10' : 'border-border active:bg-muted'}`}
          >
            <GroupLabel property={property} group={lane} ctx={ctx} />
            <Text className="text-[12px] text-muted-foreground">{lane.rows.length}</Text>
          </Pressable>
        )
      })}
    </ScrollView>
  )
}

/** A group's label: its option pill, its member, a checkbox, or "No <property>". */
function GroupLabel({ property, group, ctx }: { property: LaneProperty; group: BoardGroup; ctx: ViewContext }) {
  const { colors } = useTheme()
  if (group.id === null) return <Text className="text-[13px] text-muted-foreground">No {property.name}</Text>
  switch (property.type) {
    case 'person': {
      const name = ctx.members.get(group.id)
      return (
        <View className="flex-row items-center gap-1.5 min-w-0">
          <Avatar id={group.id} name={name ?? '?'} size={18} />
          <Text numberOfLines={1} className="text-[13px] text-foreground shrink">{name ?? 'Former member'}</Text>
        </View>
      )
    }
    case 'checkbox':
      return (
        <View className="flex-row items-center gap-1.5">
          <Icon name={group.id === 'true' ? 'checkSquare' : 'square'} size={15} color={colors.mutedForeground} />
          <Text className="text-[13px] text-foreground">{groupName(property, group, ctx.members)}</Text>
        </View>
      )
    default:
      return <OptionPill property={property} id={group.id} />
  }
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
  const name = groupName(property, group, ctx.members)
  return (
    <View accessibilityLabel={name} className="rounded-lg bg-muted/40 p-2 gap-2" style={{ width }}>
      <View className="h-7 flex-row items-center gap-2 px-1">
        <GroupLabel property={property} group={group} ctx={ctx} />
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

/**
 * "Move to…": the board's visible columns, and with sub-groups its lanes. A lane pick only selects it (the card's lane
 * at first); a column pick moves the card to that column in the selected lane, both values in one request. A card
 * with several people or options swaps the column it was opened from (`moveValues`).
 */
function MoveChoices({ rows, property, laneProperty, config, ctx, from, onPick }: {
  rows: Row[]
  property: GroupProperty
  laneProperty: LaneProperty | undefined
  config: ViewConfig
  ctx: ViewContext
  from: Cell
  onPick: (to: Cell) => void
}) {
  const [lane, setLane] = useState(from.lane)
  const columns = groupRows(rows, property, { ...config, hide_empty_groups: false }, ctx.members).filter((group) => !group.hidden)
  const lanes = laneProperty ? laneRows(rows, property, laneProperty, { ...config, hide_empty_groups: false }, ctx.members).filter((candidate) => !candidate.hidden) : []
  const choice = (owner: LaneProperty, group: BoardGroup, checked: boolean, onPress: () => void) => (
    <ChoiceRow key={group.id ?? 'none'} label={groupName(owner, group, ctx.members)} checked={checked} onPress={onPress}>
      <GroupLabel property={owner} group={group} ctx={ctx} />
    </ChoiceRow>
  )
  return (
    <SheetList>
      {laneProperty ? (
        <>
          <SheetNote>{laneProperty.name}</SheetNote>
          {lanes.map((candidate) => choice(laneProperty, candidate, candidate.id === lane, () => setLane(candidate.id)))}
          <SheetNote>{property.name}</SheetNote>
        </>
      ) : null}
      {columns.map((group) => {
        const here = group.id === from.group && lane === from.lane
        return choice(property, group, group.id === from.group, () => (here ? undefined : onPick({ group: group.id, lane })))
      })}
    </SheetList>
  )
}
