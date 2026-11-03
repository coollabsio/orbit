import { router, useLocalSearchParams } from 'expo-router'
import { useMemo, useState } from 'react'
import { Pressable, RefreshControl, SectionList, View } from 'react-native'
import { TextInput } from '../../../ui/scaled'

import { MenuButton } from '../../../components/Drawer'
import { NewTaskDialog, type NewTaskDefaults } from '../../../components/NewTaskDialog'
import { TaskRow } from '../../../components/TaskRow'
import { groupTasksByStatus, type StatusGroup } from '../../../data/tasks'
import { useOrbit } from '../../../orbit-context'
import { usePeople, useProjects, useStatuses, useTasks } from '../../../queries'
import { useTheme } from '../../../theme'
import { EmptyState, ErrorView, HeaderTitle, Icon, IconButton, LoadingView, PaneHeader, PickerSheet, SafeAreaView, StatusIcon, Text } from '../../../ui'

/** The server's task views the drawer links to, with the web page titles. */
const PRESET_TITLE: Record<string, string> = {
  mine: 'My tasks',
  overdue: 'Overdue',
  due_soon: 'Due soon',
  current_week: 'This week',
  my_week: 'My week',
}

type Sheet = 'project' | 'filter' | 'display'
type Grouping = 'status' | 'none'

export default function TasksScreen() {
  const { view: viewParam, search: searchParam, project: projectParam } = useLocalSearchParams<{ view?: string; search?: string; project?: string }>()
  const view = viewParam && viewParam in PRESET_TITLE ? viewParam : undefined
  const { user } = useOrbit()
  const { colors } = useTheme()
  const tasks = useTasks(view)
  const statuses = useStatuses()
  const projects = useProjects()
  const people = usePeople()
  // in the URL, so the drawer's project links can pick it
  const projectId = projectParam || null
  const setProjectId = (id: string | null) => router.setParams({ project: id ?? undefined })
  const [assignedToMe, setAssignedToMe] = useState(false)
  const [grouping, setGrouping] = useState<Grouping>('status')
  const [search, setSearch] = useState('')
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set())
  const [sheet, setSheet] = useState<Sheet | null>(null)
  const [creating, setCreating] = useState<NewTaskDefaults | null>(null)
  const project = projects.data?.find((item) => item.id === projectId)

  const sections = useMemo(() => {
    if (!tasks.data || !statuses.data) return []
    const needle = search.trim().toLowerCase()
    const visible = tasks.data.filter(
      (task) =>
        (!projectId || task.project_id === projectId) &&
        (!assignedToMe || task.assignee_ids.includes(user.id)) &&
        (!needle || task.title.toLowerCase().includes(needle)),
    )
    const groups: StatusGroup[] = grouping === 'status' ? groupTasksByStatus(visible, statuses.data) : [{ key: 'all', name: 'All', status: undefined, tasks: visible }]
    return groups.filter((group) => group.tasks.length > 0).map((group) => ({ ...group, data: collapsed.has(group.key) ? [] : group.tasks }))
  }, [assignedToMe, collapsed, grouping, projectId, search, statuses.data, tasks.data, user.id])

  const refresh = () => Promise.all([tasks.refetch(), statuses.refetch(), projects.refetch()])
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
        <MenuButton />
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Select project"
          onPress={() => setSheet('project')}
          className="h-8 max-w-[30%] flex-row items-center gap-[7px] px-[7px] rounded-lg active:bg-muted"
        >
          {project ? <View className="size-2 rounded-full" style={{ backgroundColor: project.color }} /> : null}
          <Text numberOfLines={1} className="text-[14px] leading-[20px] shrink text-muted-foreground">
            {project?.name ?? 'All projects'}
          </Text>
          <Icon name="chevronDown" size={14} color={colors.mutedForeground} />
        </Pressable>
        <View className="flex-1 min-w-0">
          <HeaderTitle>{view ? PRESET_TITLE[view] : 'All tasks'}</HeaderTitle>
        </View>
        <IconButton icon="filter" label="Filter" size={32} onPress={() => setSheet('filter')} color={assignedToMe ? colors.primary : undefined} />
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
    </>
  )

  const projectOptions = [{ key: 'all', label: 'All projects', selected: projectId === null }, ...(projects.data ?? []).map((item) => ({ key: item.id, label: item.name, selected: item.id === projectId, leading: <View className="size-2 rounded-full" style={{ backgroundColor: item.color }} /> }))]

  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
      {header}
      {!tasks.data || !statuses.data ? (
        tasks.error || statuses.error ? (
          <ErrorView error={tasks.error ?? statuses.error} onRetry={() => void refresh()} />
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
          refreshControl={<RefreshControl refreshing={tasks.isRefetching} onRefresh={() => void refresh()} tintColor={colors.mutedForeground} />}
          ListEmptyComponent={<EmptyState icon="tasks" title="No tasks" description={search || assignedToMe || projectId ? 'No task matches the filter.' : 'Tap + to create the first task.'} />}
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
        visible={sheet === 'filter'}
        title="Filter"
        options={[
          { key: 'everyone', label: 'All tasks', selected: !assignedToMe },
          { key: 'me', label: 'Assigned to me', selected: assignedToMe },
        ]}
        onSelect={(key) => {
          setSheet(null)
          setAssignedToMe(key === 'me')
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
          setGrouping(key === 'none' ? 'none' : 'status')
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
