import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query'
import { router, useLocalSearchParams } from 'expo-router'
import { useMemo, useState } from 'react'
import { Pressable, SectionList, useWindowDimensions, View } from 'react-native'
import { TextInput } from '../../../ui/scaled'

import { NewTaskDialog, type NewTaskDefaults } from '../../../components/NewTaskDialog'
import { TaskRow } from '../../../components/TaskRow'
import { FilterBar } from '../../../components/tasks/FilterBar'
import { keys } from '../../../data/keys'
import { groupTasksByStatus, type StatusGroup } from '../../../data/tasks'
import {
  countConditions,
  DEFAULT_DISPLAY,
  effectiveFilter,
  emptyFilter,
  isTaskPreset,
  pageKeyFor,
  PRESET_LABEL,
  queryAllTasks,
  saveViewPreference,
  taskQueryBody,
  viewPreference,
  type FilterGroup,
  type FilterOptions,
  type ViewState,
} from '../../../data/tasks-filter'
import { useOrbit } from '../../../orbit-context'
import { useLabels, usePeople, useProjects, useStatuses } from '../../../queries'
import { useTheme } from '../../../theme'
import { EmptyState, ErrorView, HeaderTitle, Icon, IconButton, LoadingView, PaneHeader, PullRefresh, PickerSheet, SafeAreaView, StatusIcon, Text } from '../../../ui'

type Sheet = 'project' | 'display'
type Grouping = 'status' | 'none'

export default function TasksScreen() {
  const { view: viewParam, search: searchParam, project: projectParam } = useLocalSearchParams<{ view?: string; search?: string; project?: string }>()
  const preset = isTaskPreset(viewParam) ? viewParam : undefined
  const { client, workspaceId, user } = useOrbit()
  const { colors } = useTheme()
  const { width: windowWidth } = useWindowDimensions()
  const queryClient = useQueryClient()
  const statuses = useStatuses()
  const projects = useProjects()
  const labels = useLabels()
  const people = usePeople()
  // in the URL, so the drawer's project links can pick it
  const projectId = projectParam || null
  const setProjectId = (id: string | null) => router.setParams({ project: id ?? undefined })
  // the filter and display are stored per page on the server, as the web stores them, so both apps show the same filter
  const pageKey = pageKeyFor(preset, projectId)
  const preferenceKey = ['view-preference', workspaceId, pageKey] as const
  const preference = useQuery({ queryKey: preferenceKey, queryFn: () => viewPreference(client, workspaceId, pageKey) })
  const state: ViewState = preference.data ?? { filter: emptyFilter(), display: DEFAULT_DISPLAY }
  const saveState = (next: ViewState) => {
    queryClient.setQueryData(preferenceKey, next)
    saveViewPreference(client, workspaceId, pageKey, next).catch(() => void queryClient.invalidateQueries({ queryKey: preferenceKey }))
  }
  const setFilter = (filter: FilterGroup) => saveState({ ...state, filter })
  const body = taskQueryBody(effectiveFilter(state.filter, preset, projectId), state.display)
  const tasks = useQuery({
    queryKey: [...keys.tasks(workspaceId), 'query', body],
    queryFn: () => queryAllTasks(client, workspaceId, body),
    enabled: preference.isSuccess,
    placeholderData: keepPreviousData,
  })
  const grouping: Grouping = state.display.group_by === 'none' ? 'none' : 'status'
  const [adding, setAdding] = useState(false)
  const [search, setSearch] = useState('')
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set())
  const [sheet, setSheet] = useState<Sheet | null>(null)
  const [creating, setCreating] = useState<NewTaskDefaults | null>(null)
  const project = projects.data?.find((item) => item.id === projectId)
  const filterCount = countConditions(state.filter)
  const filterOptions: FilterOptions = {
    statuses: projectId ? (statuses.data ?? []).filter((status) => status.project_id === projectId) : (statuses.data ?? []),
    members: people,
    labels: labels.data ?? [],
    projectId,
    projects: projectId ? (projects.data ?? []).filter((item) => item.id === projectId) : (projects.data ?? []),
    currentUserId: user.id,
  }

  const sections = useMemo(() => {
    if (!tasks.data || !statuses.data) return []
    const needle = search.trim().toLowerCase()
    const visible = needle ? tasks.data.filter((task) => task.title.toLowerCase().includes(needle)) : tasks.data
    const groups: StatusGroup[] = grouping === 'status' ? groupTasksByStatus(visible, statuses.data) : [{ key: 'all', name: 'All', status: undefined, tasks: visible }]
    return groups.filter((group) => group.tasks.length > 0).map((group) => ({ ...group, data: collapsed.has(group.key) ? [] : group.tasks }))
  }, [collapsed, grouping, search, statuses.data, tasks.data])

  const refresh = () => Promise.all([preference.refetch(), tasks.refetch(), statuses.refetch(), projects.refetch()])
  const toggle = (key: string) =>
    setCollapsed((current) => {
      const next = new Set(current)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  const newTask = (statusId?: string) => setCreating({ projectId: projectId ?? undefined, statusId })

  const header = (
    <>
      <PaneHeader className="flex-wrap">
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Select project"
          onPress={() => setSheet('project')}
          className="h-8 flex-row items-center gap-[7px] px-[7px] rounded-lg active:bg-muted"
          // the web's max-w-[30vw]: a share of the window, not of the padded header
          style={{ maxWidth: windowWidth * 0.3 }}
        >
          {project ? <View className="size-2 rounded-full" style={{ backgroundColor: project.color }} /> : null}
          <Text numberOfLines={1} className="text-[14px] leading-[20px] shrink text-muted-foreground">
            {project?.name ?? 'All projects'}
          </Text>
          <Icon name="chevronDown" size={14} color={colors.mutedForeground} />
        </Pressable>
        <View className="flex-1 min-w-0">
          <HeaderTitle>{preset ? PRESET_LABEL[preset] : 'All tasks'}</HeaderTitle>
        </View>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Filter tasks"
          onPress={() => setAdding(true)}
          // as on the web, the count sits beside the icon and the pair centers on the 32px button
          className={`size-8 flex-row items-center justify-center gap-1.5 rounded-lg ${filterCount > 0 ? 'bg-primary/10' : 'active:bg-muted'}`}
        >
          <View className="shrink-0">
            <Icon name="filter" size={16} color={filterCount > 0 ? colors.primary : colors.foreground} />
          </View>
          {filterCount > 0 ? (
            <View className="h-4 min-w-4 shrink-0 items-center justify-center rounded-full bg-primary px-1">
              <Text className="font-noto-medium text-[10px] leading-[12px] text-primary-foreground">{filterCount}</Text>
            </View>
          ) : null}
        </Pressable>
        <IconButton icon="display" label="Display" size={32} onPress={() => setSheet('display')} />
        <IconButton icon="plus" label="New task" size={32} variant="primary" onPress={() => newTask()} />
        <View
          className="h-8.5 mt-1 basis-full flex-row items-center border border-input rounded-lg bg-input/30"
        >
          <View className="w-5.5 items-end">
            <Icon name="search" size={14} color={colors.mutedForeground} />
          </View>
          <TextInput
            accessibilityLabel="Search tasks"
            placeholder="Search tasks"
            placeholderTextColor={colors.mutedForeground}
            selectionColor={colors.primary}
            autoFocus={searchParam === '1'}
            value={search}
            onChangeText={setSearch}
            returnKeyType="search"
            className="flex-1 h-8 pl-2 pr-2.5 font-noto text-[16px] text-foreground"
          />
        </View>
      </PaneHeader>
      <FilterBar filter={state.filter} options={filterOptions} onChange={setFilter} presetLabel={preset ? PRESET_LABEL[preset] : null} adding={adding} onAddingChange={setAdding} />
    </>
  )

  const projectOptions = [{ key: 'all', label: 'All projects', selected: projectId === null }, ...(projects.data ?? []).map((item) => ({ key: item.id, label: item.name, selected: item.id === projectId, leading: <View className="size-2 rounded-full" style={{ backgroundColor: item.color }} /> }))]

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      {header}
      {!tasks.data || !statuses.data ? (
        preference.error || tasks.error || statuses.error ? (
          <ErrorView error={preference.error ?? tasks.error ?? statuses.error} onRetry={() => void refresh()} />
        ) : (
          <LoadingView />
        )
      ) : (
        <SectionList
          sections={sections}
          keyExtractor={(task) => task.id}
          stickySectionHeadersEnabled={false}
          keyboardShouldPersistTaps="handled"
          contentContainerClassName={sections.length === 0 ? 'grow' : 'pb-6'}
          refreshControl={<PullRefresh onRefresh={() => refresh()} />}
          ListEmptyComponent={<EmptyState icon="tasks" title="No tasks found" description="No tasks match the current filters. Try clearing a filter or create a new task." />}
          renderSectionHeader={({ section }) =>
            grouping === 'none' ? null : (
              <GroupHeader group={section} count={section.tasks.length} collapsed={collapsed.has(section.key)} onToggle={() => toggle(section.key)} onAdd={() => newTask(section.status?.id)} />
            )
          }
          renderItem={({ item }) => (
            <TaskRow
              task={item}
              status={statuses.data.find((status) => status.id === item.status_id)}
              assignee={people.find((person) => person.id === item.assignee_ids[0])}
              onPress={() => router.push(`/tasks/${item.id}`)}
            />
          )}
        />
      )}
      {creating ? <NewTaskDialog defaults={creating} onClose={() => setCreating(null)} /> : null}
      <PickerSheet
        visible={sheet === 'project'}
        title="Project"
        options={projectOptions}
        onSelect={(key) => {
          setSheet(null)
          setProjectId(key === 'all' ? null : key)
        }}
        onClose={() => setSheet(null)}
      />
      <PickerSheet
        visible={sheet === 'display'}
        title="Grouping"
        options={[
          { key: 'status', label: 'Status', selected: grouping === 'status' },
          { key: 'none', label: 'No grouping', selected: grouping === 'none' },
        ]}
        onSelect={(key) => {
          setSheet(null)
          saveState({ ...state, display: { ...state.display, group_by: key === 'none' ? 'none' : 'status' } })
        }}
        onClose={() => setSheet(null)}
      />
    </SafeAreaView>
  )
}

/** The web group row: chevron, status, name, count, and + on the card surface. */
function GroupHeader({ group, count, collapsed, onToggle, onAdd }: { group: StatusGroup; count: number; collapsed: boolean; onToggle: () => void; onAdd: () => void }) {
  const { colors } = useTheme()
  return (
    <View className="h-9 flex-row items-center pl-1.5 pr-2 bg-card border-b border-border">
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={collapsed ? `Expand ${group.name}` : `Collapse ${group.name}`}
        onPress={onToggle}
        hitSlop={8}
        className="size-5 rounded-md items-center justify-center bg-muted"
      >
        <View className={collapsed ? '-rotate-90' : undefined}>
          <Icon name="chevronDown" size={12} color={colors.foreground} />
        </View>
      </Pressable>
      <View className="ml-2">
        <StatusIcon status={group.status} size={14} />
      </View>
      <Text className="font-noto-semibold text-[12px] leading-[16px] ml-2 text-muted-foreground">{group.name}</Text>
      <Text className="text-[12px] leading-[16px] ml-2 flex-1 text-muted-foreground/70">{count}</Text>
      <IconButton icon="plus" label={`New task in ${group.name}`} size={24} iconSize={12} color={colors.mutedForeground} onPress={onAdd} />
    </View>
  )
}
