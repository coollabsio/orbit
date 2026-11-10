import { type Href, router } from 'expo-router'
import * as WebBrowser from 'expo-web-browser'
import { type ReactNode, useMemo, useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import {
  checkStates,
  IMPORT_STATUS_LABELS,
  type ImportStatus,
  importStatusTone,
  nodeTitle,
  PRIVATE_SPACE,
  selectAll,
  selectedCount,
  type SelectionTree,
  type SpaceKey,
  teamspaceSpace,
  TOP_LEVEL,
  toggleNode,
  type TreeNode,
  visibleRows,
} from '../../data/import-api'
import { relativeTime } from '../../data/format'
import type { ImportDestination } from '../../hooks/imports'
import { alpha, presenceColors, useTheme } from '../../theme'
import { Button, Icon, IconButton, type IconName, KeyboardSpace, PaneHeader, SafeAreaView, Spinner, Text } from '../../ui'
import { TextInput } from '../../ui/scaled'
import { Dialog } from '../settings/Dialog'
import { Badge, Select } from '../settings/Shell'

/** The card that lists the sources; every flow's back button returns there. */
export const IMPORT_HOME = '/settings/import'

/** An import pane: the 48px title row (back, title, detail, actions) over the step. */
export function ImportScreen({ title, detail, actions, children }: { title: string; detail?: string | null; actions?: ReactNode; children: ReactNode }) {
  const { colors } = useTheme()
  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 gap-2 border-b border-border px-3">
        <IconButton icon="back" label="Back to import" color={alpha(colors.mutedForeground, 0.7)} onPress={() => (router.canGoBack() ? router.back() : router.navigate(IMPORT_HOME))} />
        <Text accessibilityRole="header" numberOfLines={1} className="shrink-0 font-noto-semibold text-[13px]">
          {title}
        </Text>
        {detail ? (
          <Text numberOfLines={1} className="shrink text-[13px] text-muted-foreground">
            {detail}
          </Text>
        ) : null}
        <View className="flex-1" />
        {actions}
      </PaneHeader>
      <KeyboardSpace className="flex-1">
        {children}
      </KeyboardSpace>
    </SafeAreaView>
  )
}

/** The scrolling column of a step (the web's `ImportBody` at phone width). */
export function ImportBody({ children }: { children: ReactNode }) {
  return (
    <ScrollView className="flex-1" contentContainerClassName="p-4 gap-6" keyboardShouldPersistTaps="handled">
      {children}
    </ScrollView>
  )
}

/** The bordered "Connect …" or "Upload your vault" box: the title over its explanation (`gap`), then the form. */
export function ConnectSection({ title, intro, gap = 'gap-1', children }: { title: string; intro: ReactNode; gap?: 'gap-1' | 'gap-2'; children: ReactNode }) {
  return (
    <View className="gap-4 rounded-lg border border-border p-5">
      <View className={gap}>
        <Text accessibilityRole="header" className="font-noto-semibold text-[14px] leading-[20px]">
          {title}
        </Text>
        {intro}
      </View>
      {children}
    </View>
  )
}

/** A muted paragraph of the connect box; nest `Strong` and `ExternalLink` in it. */
export function Paragraph({ children }: { children: ReactNode }) {
  return <Text className="text-[14px] leading-[20px] text-muted-foreground">{children}</Text>
}

export function Strong({ children }: { children: ReactNode }) {
  return <Text className="font-noto-medium text-[14px] leading-[20px] text-foreground">{children}</Text>
}

/** A link to the source's settings, opened in the in-app browser. */
export function ExternalLink({ url, children }: { url: string; children?: ReactNode }) {
  return (
    <Text accessibilityRole="link" onPress={() => void WebBrowser.openBrowserAsync(url).catch(() => undefined)} className="text-[14px] leading-[20px] text-primary underline">
      {children ?? url}
    </Text>
  )
}

/** A failed connect, with a link to the import already running when that is why. */
export function ConnectError({ message, running }: { message: string; running?: Href | null }) {
  return (
    <Text accessibilityRole="alert" className="text-[14px] leading-[20px] text-destructive">
      {message}
      {running ? (
        <>
          {' '}
          <Text accessibilityRole="link" onPress={() => router.push(running)} className="text-[14px] leading-[20px] text-primary underline">
            Open the running import
          </Text>
        </>
      ) : null}
    </Text>
  )
}

export function ProgressBar({ done, total, label }: { done: number; total: number; label: string }) {
  const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0
  return (
    <View accessibilityRole="progressbar" accessibilityLabel={label} accessibilityValue={{ min: 0, max: total, now: done }} className="h-2 w-full overflow-hidden rounded-full bg-muted">
      <View className="h-full rounded-full bg-primary" style={{ width: `${percent}%` }} />
    </View>
  )
}

export function Stat({ label, value }: { label: string; value: number }) {
  return (
    <View className="flex-1 gap-0.5 rounded-lg border border-border px-3 py-2">
      <Text className="font-noto-semibold text-[18px] leading-[28px] tabular-nums">{value}</Text>
      <Text className="text-[12px] leading-[16px] text-muted-foreground">{label}</Text>
    </View>
  )
}

/** A quiet 12px section heading ("Recent imports", "Conversion notes"). */
export function SectionLabel({ children }: { children: ReactNode }) {
  return <Text className="font-noto-medium text-[12px] leading-[16px] text-muted-foreground">{children}</Text>
}

interface ListedImport {
  id: string
  status: ImportStatus
  progress: { done: number; total: number }
  created_at: string
}

/** The member's recent imports, each opening its own screen. */
export function RecentImports<T extends ListedImport>({ imports, name, unit, href }: { imports: T[] | undefined; name: (item: T) => string; unit: string; href: (item: T) => Href }) {
  if (!imports?.length) return null
  return (
    <View accessibilityLabel="Recent imports" className="gap-2">
      <SectionLabel>Recent imports</SectionLabel>
      <View className="border-t border-border">
        {imports.map((item) => (
          <Pressable key={item.id} accessibilityRole="link" onPress={() => router.push(href(item))} className="min-h-10 flex-row items-center gap-3 border-b border-border px-1 py-1.5 active:bg-sidebar-accent/50">
            <Text numberOfLines={1} className="min-w-0 flex-1 text-[13px]">
              {name(item)}
            </Text>
            {item.progress.total > 0 ? (
              <Text className="shrink-0 text-[12px] text-muted-foreground">
                {item.progress.done} of {item.progress.total} {unit}
              </Text>
            ) : null}
            <Badge label={IMPORT_STATUS_LABELS[item.status]} tone={importStatusTone(item.status)} />
            <Text numberOfLines={1} className="w-12 shrink-0 text-right text-[12px] text-muted-foreground">
              {relativeTime(item.created_at)}
            </Text>
          </Pressable>
        ))}
      </View>
    </View>
  )
}

/** The web `Checkbox`: a 16px box, filled pink with a check (or a dash when only part is chosen). */
export function Checkbox({ checked, indeterminate = false, label, onPress, disabled = false }: { checked: boolean; indeterminate?: boolean; label: string; onPress: () => void; disabled?: boolean }) {
  const { colors } = useTheme()
  const on = checked || indeterminate
  return (
    <Pressable
      accessibilityRole="checkbox"
      accessibilityLabel={label}
      accessibilityState={{ checked: indeterminate ? 'mixed' : checked, disabled }}
      disabled={disabled}
      onPress={onPress}
      hitSlop={10}
      className={`size-4 shrink-0 items-center justify-center rounded-[4px] border ${on ? 'border-primary bg-primary' : 'border-input dark:bg-input/30'} ${disabled ? 'opacity-50' : ''}`}
    >
      {on ? <Icon name={indeterminate ? 'minus' : 'check'} size={14} color={colors.primaryForeground} /> : null}
    </Pressable>
  )
}

/** The web's filter `InputGroup`: a search icon and the field. */
export function FilterInput({ value, onChange, placeholder }: { value: string; onChange: (value: string) => void; placeholder: string }) {
  const { colors, scheme } = useTheme()
  return (
    <View className="h-8 w-full flex-row items-center gap-2 rounded-lg border border-border bg-foreground/[0.02] pl-2.5 dark:bg-input/30">
      <Icon name="search" size={14} color={alpha(colors.mutedForeground, 0.7)} />
      <TextInput
        accessibilityLabel={placeholder}
        placeholder={placeholder}
        placeholderTextColor={colors.mutedForeground}
        selectionColor={colors.primary}
        keyboardAppearance={scheme}
        autoCapitalize="none"
        autoCorrect={false}
        value={value}
        onChangeText={onChange}
        className="h-full min-w-0 flex-1 pr-2.5 font-noto text-foreground"
      />
    </View>
  )
}

/** The amber "only the first N are listed" strip above a chooser. */
export function WarningStrip({ children }: { children: ReactNode }) {
  return (
    <View accessibilityRole="alert" className="gap-1 border-b border-border bg-amber-500/5 px-4 py-2">
      {children}
    </View>
  )
}

export function WarningLine({ children }: { children: ReactNode }) {
  return (
    <View className="flex-row items-start gap-2">
      <View className="mt-px">
        {/* the web's amber-600; amber-500 is the nearest token the app has */}
        <Icon name="warning" size={14} color={presenceColors.idle} />
      </View>
      <Text className="flex-1 text-[12px] leading-[16px] text-foreground">{children}</Text>
    </View>
  )
}

/**
 * The bottom bar of a chooser: what the import does (or the destination fields) on the left, and the error and the
 * Import button on the right.
 */
export function ChooserFooter({ children, error, label, disabled, onImport }: { children: ReactNode; error: string | null; label: string; disabled: boolean; onImport: () => void }) {
  return (
    <View className="flex-row flex-wrap items-end gap-3 border-t border-border px-3 py-3">
      {children}
      <View className="flex-1" />
      <View className="items-end gap-1.5">
        {error ? (
          <Text accessibilityRole="alert" className="max-w-96 text-right text-[12px] leading-[16px] text-destructive">
            {error}
          </Text>
        ) : null}
        <Button label={label} disabled={disabled} onPress={onImport} />
      </View>
    </View>
  )
}

/** A chooser row: the checkbox and the name, which toggles the row too. */
export function ChoiceRow({ checked, label, onToggle, indent = false, children }: { checked: boolean; label: string; onToggle: () => void; indent?: boolean; children?: ReactNode }) {
  return (
    <Pressable onPress={onToggle} className={`h-8 flex-row items-center gap-2 rounded-md pr-2 active:bg-sidebar-accent/50 ${indent ? 'pl-8' : 'pl-2'}`}>
      <Checkbox label={`Select ${label}`} checked={checked} onPress={onToggle} />
      {children}
    </Pressable>
  )
}

/** The filter and the "Select all" box of a task import's chooser. */
export function ChooserToolbar({ query, onQuery, placeholder, allLabel, checked, indeterminate, disabled, onToggleAll }: { query: string; onQuery: (value: string) => void; placeholder: string; allLabel: string; checked: boolean; indeterminate: boolean; disabled: boolean; onToggleAll: () => void }) {
  return (
    <View className="flex-row flex-wrap items-center gap-3 border-b border-border px-3 py-2">
      <FilterInput value={query} onChange={onQuery} placeholder={placeholder} />
      <Pressable disabled={disabled} onPress={onToggleAll} className="flex-row items-center gap-2">
        <Checkbox label={allLabel} checked={checked} indeterminate={indeterminate} disabled={disabled} onPress={onToggleAll} />
        <Text className="text-[13px]">Select all</Text>
      </Pressable>
    </View>
  )
}

/** The filter, "N pages selected" and the tri-state tree of a Docs import's "Choose pages" step. */
export function ImportTreePicker<T extends TreeNode & { child_count: number }>({ tree, selected, onSelectedChange, label, empty, icon }: { tree: SelectionTree<T>; selected: ReadonlySet<string>; onSelectedChange: (selected: ReadonlySet<string>) => void; label: string; empty: string; icon: (node: T) => ReactNode }) {
  const { colors } = useTheme()
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set())
  const [query, setQuery] = useState('')
  const states = useMemo(() => checkStates(tree, selected), [tree, selected])
  const rows = useMemo(() => visibleRows(tree, expanded, query), [tree, expanded, query])
  const count = selectedCount(tree, selected)
  const filtering = query.trim() !== ''
  const toggle = (id: string) => onSelectedChange(toggleNode(tree, selected, id))
  const toggleExpanded = (id: string) =>
    setExpanded((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  const muted = alpha(colors.mutedForeground, 0.7)
  return (
    <>
      <View className="flex-row flex-wrap items-center gap-2 border-b border-border px-3 py-2">
        <FilterInput value={query} onChange={setQuery} placeholder="Filter pages" />
        <View className="flex-1" />
        <Text className="text-[12px] text-muted-foreground tabular-nums">
          {count} {count === 1 ? 'page' : 'pages'} selected
        </Text>
        <Button label="Select all" variant="ghost" size="sm" onPress={() => onSelectedChange(selectAll(tree))} />
        <Button label="Clear" variant="ghost" size="sm" onPress={() => onSelectedChange(new Set())} />
      </View>
      <ScrollView accessibilityLabel={label} className="flex-1" contentContainerClassName="p-2" keyboardShouldPersistTaps="handled">
        {tree.ids.length === 0 ? (
          <Text className="p-4 text-[14px] leading-[20px] text-muted-foreground">{empty}</Text>
        ) : rows.length === 0 ? (
          <Text className="p-4 text-[14px] leading-[20px] text-muted-foreground">No pages match “{query.trim()}”.</Text>
        ) : (
          rows.slice(0, MAX_ROWS).map((row) => {
            const state = states.get(row.id) ?? 'unchecked'
            const title = nodeTitle(row.node)
            return (
              <Pressable
                key={row.id}
                accessibilityState={{ selected: state === 'checked', expanded: row.hasChildren ? row.expanded : undefined }}
                onPress={() => toggle(row.id)}
                className="h-8 min-w-0 flex-row items-center gap-1.5 rounded-md pr-2 active:bg-sidebar-accent/50"
                style={{ paddingLeft: 4 + row.depth * 18 }}
              >
                {row.hasChildren && !filtering ? (
                  <Pressable
                    accessibilityRole="button"
                    accessibilityLabel={`${row.expanded ? 'Collapse' : 'Expand'} ${title}`}
                    onPress={() => toggleExpanded(row.id)}
                    hitSlop={6}
                    className="size-5 items-center justify-center rounded-md active:bg-muted"
                    style={{ transform: [{ rotate: row.expanded ? '90deg' : '0deg' }] }}
                  >
                    <Icon name="chevronRight" size={12} color={muted} />
                  </Pressable>
                ) : (
                  <View className="size-5" />
                )}
                <Checkbox label={`Select ${title}`} checked={state === 'checked'} indeterminate={state === 'indeterminate'} onPress={() => toggle(row.id)} />
                <View className="w-[18px] items-center justify-center">{icon(row.node)}</View>
                <Text numberOfLines={1} className={`min-w-0 flex-1 text-[13px] ${row.match ? 'font-noto-medium' : ''}`}>
                  {title}
                </Text>
                {row.node.child_count > 0 ? <Text className="text-[12px] text-muted-foreground/70">{row.node.child_count}</Text> : null}
              </Pressable>
            )
          })
        )}
        {rows.length > MAX_ROWS ? (
          <Text className="p-3 text-[12px] text-muted-foreground">
            Showing the first {MAX_ROWS} of {rows.length} rows. Filter to narrow the list.
          </Text>
        ) : null}
      </ScrollView>
    </>
  )
}

/** Rendering thousands of search hits at once is slow and useless; ask for a narrower filter instead. */
const MAX_ROWS = 1000

/** A tree row's icon (15px, muted). */
export function NodeIcon({ name }: { name: IconName }) {
  const { colors } = useTheme()
  return <Icon name={name} size={15} color={alpha(colors.mutedForeground, 0.7)} />
}

/** The "Import into" and "Inside page" selects of a Docs import. */
export function DestinationFields({ destination }: { destination: ImportDestination }) {
  const spaces = [...(destination.teamspaces ?? []).map((teamspace) => ({ value: teamspaceSpace(teamspace.id) as string, label: teamspace.name })), { value: PRIVATE_SPACE as string, label: 'Private' }]
  const parents = [{ value: TOP_LEVEL, label: 'Top level' }, ...destination.parentOptions.map(({ page, depth }) => ({ value: page.id, label: `${'  '.repeat(depth)}${page.title.trim() || 'Untitled'}` }))]
  return (
    <>
      <View className="w-48 gap-1.5">
        <Text className="font-noto-medium text-[12px] leading-[16px] text-muted-foreground">Import into</Text>
        <Select label="Import into" value={destination.space as string} options={spaces} onChange={(value) => destination.chooseSpace(value as SpaceKey)} />
      </View>
      <View className="w-56 gap-1.5">
        <Text className="font-noto-medium text-[12px] leading-[16px] text-muted-foreground">Inside page</Text>
        <Select label="Inside page" value={destination.parent} options={parents} onChange={destination.chooseParent} />
      </View>
    </>
  )
}

/** The web's `confirmAction` for cancelling an import: "Keep going" or "Cancel import". */
export function CancelDialog({ request, onClose }: { request: { title: string; description: string; onConfirm: () => void } | null; onClose: () => void }) {
  return (
    <Dialog
      visible={request !== null}
      title={request?.title ?? ''}
      description={request?.description}
      onClose={onClose}
      footer={
        <>
          <Button label="Keep going" variant="outline" onPress={onClose} />
          <Button
            label="Cancel import"
            variant="destructive"
            onPress={() => {
              onClose()
              request?.onConfirm()
            }}
          />
        </>
      }
    />
  )
}

/** A centered spinner filling the pane. */
export function Loading() {
  return (
    <View className="flex-1 items-center justify-center">
      <Spinner />
    </View>
  )
}

/** The imported top-level pages that still exist, with "Open imported pages". */
export function ImportedPages({ rootIds, pages }: { rootIds: string[]; pages: readonly { id: string; title: string }[] | undefined }) {
  if (rootIds.length === 0) return null
  const titles = new Map((pages ?? []).map((page) => [page.id, page.title]))
  return (
    <View accessibilityLabel="Imported pages" className="gap-2">
      <View className="flex-row items-center gap-2">
        <SectionLabel>Imported pages</SectionLabel>
        <View className="flex-1" />
        <Button label="Open imported pages" size="sm" onPress={() => router.push(`/docs/${rootIds[0]}`)} />
      </View>
      <View className="border-t border-border">
        {rootIds.slice(0, 50).map((id) => {
          const title = titles.get(id)
          return (
            <Pressable key={id} accessibilityRole="link" onPress={() => router.push(`/docs/${id}`)} className="h-9 flex-row items-center gap-2 border-b border-border px-1 active:bg-sidebar-accent/50">
              <NodeIcon name="docs" />
              <Text numberOfLines={1} className="flex-1 text-[13px]">
                {title === undefined ? 'Imported page' : title.trim() || 'Untitled'}
              </Text>
            </Pressable>
          )
        })}
      </View>
      {rootIds.length > 50 ? <Text className="text-[12px] text-muted-foreground">and {rootIds.length - 50} more</Text> : null}
    </View>
  )
}

/** The projects a task import created, each opening its tasks. */
export function ImportedProjects({ ids, projects }: { ids: string[]; projects: readonly { id: string; name: string; color: string }[] | undefined }) {
  if (ids.length === 0) return null
  const byId = new Map((projects ?? []).map((project) => [project.id, project]))
  return (
    <View accessibilityLabel="Imported projects" className="gap-2">
      <SectionLabel>Imported projects</SectionLabel>
      <View className="border-t border-border">
        {ids.map((id) => {
          const project = byId.get(id)
          return (
            <Pressable key={id} accessibilityRole="link" onPress={() => router.push(`/tasks?project=${id}`)} className="h-9 flex-row items-center gap-2 border-b border-border px-1 active:bg-sidebar-accent/50">
              {/* the project's own color, from data */}
              <View className="mx-[3px] size-2 rounded-[2px]" style={project ? { backgroundColor: project.color } : undefined} />
              <Text numberOfLines={1} className="flex-1 text-[13px]">
                {project?.name ?? 'Imported project'}
              </Text>
            </Pressable>
          )
        })}
      </View>
    </View>
  )
}
