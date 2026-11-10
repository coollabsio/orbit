import { router } from 'expo-router'
import { useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'
import { TextInput } from '../../../../ui/scaled'

import { filterTrash, trashIdentifier } from '@orbit/api/trash'

import { ConfirmDialog, type ConfirmRequest } from '../../../../components/settings/Dialog'
import type { ProjectRecord, TaskRecord } from '../../../../data/orbit'
import { useTaskTrash, useTaskTrashWrites } from '../../../../hooks/tasks'
import { useOrbit } from '../../../../orbit-context'
import { useProjects } from '../../../../queries'
import { errorMessage } from '../../../../session-context'
import { alpha, useTheme } from '../../../../theme'
import { Button, EmptyState, ErrorView, Icon, IconButton, LoadingView, PaneHeader, PullRefresh, SafeAreaView, Text } from '../../../../ui'

const plural = (count: number, noun: string) => `${count} ${noun}${count === 1 ? '' : 's'}`

/**
 * The web's Task trash: trashed projects first, then the trashed tasks of live projects, with a search and Restore.
 * A row opens its read-only preview. Workspace owners and admins (`pages.purge`, the server's `can_purge_task`) also
 * get Delete forever and Empty trash.
 */
export default function TaskTrashScreen() {
  const { colors } = useTheme()
  const { workspace } = useOrbit()
  const canPurge = workspace.permissions.includes('pages.purge')
  const { projects, tasks } = useTaskTrash()
  const liveProjects = useProjects()
  const writes = useTaskTrashWrites()
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [search, setSearch] = useState('')
  const busy = writes.restoreProject.isPending || writes.restoreTask.isPending || writes.purgeProject.isPending || writes.purgeTask.isPending || writes.empty.isPending
  const failure = writes.restoreProject.error ?? writes.restoreTask.error ?? writes.purgeProject.error ?? writes.purgeTask.error ?? writes.empty.error
  const count = (projects.data?.length ?? 0) + (tasks.data?.length ?? 0)
  const keyOf = (projectId: string) => liveProjects.data?.find((project) => project.id === projectId)?.key
  const shown = filterTrash(projects.data ?? [], tasks.data ?? [], search, keyOf)
  const refetch = () => Promise.all([projects.refetch(), tasks.refetch()])

  const requestPurge = (project: ProjectRecord) =>
    setConfirm({
      title: `Delete “${project.name}” forever?`,
      description: 'The project and all its tasks are deleted permanently. They can’t be restored.',
      confirmLabel: 'Delete forever',
      danger: true,
      onConfirm: () => writes.purgeProject.mutate(project.id),
    })
  const requestPurgeTask = (task: TaskRecord) => {
    const below = (task as { trashed_descendant_count?: number }).trashed_descendant_count ?? 0
    setConfirm({
      title: `Delete “${task.title || 'Untitled task'}” forever?`,
      description: below > 0 ? `The task and its ${plural(below, 'sub-issue')} are deleted permanently. They can’t be restored.` : 'The task is deleted permanently. It can’t be restored.',
      confirmLabel: 'Delete forever',
      danger: true,
      onConfirm: () => writes.purgeTask.mutate(task.id),
    })
  }
  const requestEmpty = () => {
    const parts = [
      projects.data?.length ? `${plural(projects.data.length, 'project')} with all their tasks` : null,
      tasks.data?.length ? plural(tasks.data.length, 'task') : null,
    ].filter(Boolean)
    setConfirm({
      title: 'Empty trash?',
      description: `${parts.join(' and ')} will be deleted forever. They can’t be restored.`,
      confirmLabel: 'Empty trash',
      danger: true,
      onConfirm: () => writes.empty.mutate(),
    })
  }

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3 flex-wrap">
        <IconButton icon="back" label="Back to tasks" color={alpha(colors.mutedForeground, 0.7)} onPress={() => (router.canGoBack() ? router.back() : router.navigate('/tasks'))} />
        <Text numberOfLines={1} className="font-noto-semibold text-[13px] flex-1 min-w-0">
          Trash
        </Text>
        {canPurge && count > 0 ? (
          // the web's ghost `sm` button with `text-destructive`
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Empty trash"
            disabled={busy}
            onPress={requestEmpty}
            className={`h-7 flex-row items-center rounded-md px-2.5 active:bg-muted dark:active:bg-muted/50 ${busy ? 'opacity-50' : ''}`}
          >
            <Text className="font-noto-medium text-[12.8px] leading-[18px] text-destructive">Empty trash</Text>
          </Pressable>
        ) : null}
        {count > 0 ? (
          <View className="h-8.5 mb-1.5 basis-full flex-row items-center border border-input rounded-lg bg-input/30">
            <View className="w-5.5 items-end">
              <Icon name="search" size={14} color={colors.mutedForeground} />
            </View>
            <TextInput
              accessibilityLabel="Search trash"
              placeholder="Search trash"
              placeholderTextColor={colors.mutedForeground}
              selectionColor={colors.primary}
              value={search}
              onChangeText={setSearch}
              returnKeyType="search"
              className="flex-1 h-8 pl-2 pr-2.5 font-noto text-[16px] text-foreground"
            />
          </View>
        ) : null}
      </PaneHeader>
      {failure ? <Text className="text-[13px] mx-4 mb-2 text-destructive">{errorMessage(failure)}</Text> : null}
      {!projects.data || !tasks.data ? (
        projects.isError || tasks.isError ? (
          <ErrorView error={projects.error ?? tasks.error} onRetry={() => void refetch()} />
        ) : (
          <LoadingView />
        )
      ) : count === 0 ? (
        <EmptyState icon="trash" title="Trash is empty" description="Deleted projects and tasks appear here until restored." />
      ) : shown.projects.length + shown.tasks.length === 0 ? (
        <EmptyState icon="search" title="No results" description="No deleted project or task matches your search." />
      ) : (
        <ScrollView contentContainerClassName="pb-6" refreshControl={<PullRefresh onRefresh={refetch} />}>
          {shown.projects.map((project) => (
            <View key={project.id} className="min-h-10 flex-row items-center gap-2.5 border-b border-border px-3 py-1.5">
              <Pressable accessibilityRole="link" accessibilityLabel={`Open ${project.name}`} onPress={() => router.push(`/tasks/trash/project/${project.id}`)} className="flex-1 min-w-0 flex-row items-center gap-2.5 self-stretch">
                <Icon name="folder" size={15} color={project.color || alpha(colors.mutedForeground, 0.7)} />
                <Text numberOfLines={1} className="flex-1 min-w-0 text-[13px]">
                  {project.name}
                </Text>
              </Pressable>
              <Button label="Restore" icon="refresh" size="sm" variant="outline" disabled={busy} onPress={() => writes.restoreProject.mutate(project)} />
              {canPurge ? (
                <IconButton icon="trash" iconSize={14} label={`Delete ${project.name} forever`} color={colors.destructive} disabled={busy} onPress={() => requestPurge(project)} />
              ) : null}
            </View>
          ))}
          {shown.tasks.map((task) => {
            const below = (task as { trashed_descendant_count?: number }).trashed_descendant_count ?? 0
            const id = trashIdentifier(task, keyOf)
            return (
              <View key={task.id} className="min-h-10 flex-row items-center gap-2.5 border-b border-border px-3 py-1.5">
                <Pressable accessibilityRole="link" accessibilityLabel={`Open ${task.title || 'Untitled task'}`} onPress={() => router.push(`/tasks/trash/task/${task.id}`)} className="flex-1 min-w-0 flex-row items-center gap-2.5 self-stretch">
                  <Icon name="tasks" size={15} color={alpha(colors.mutedForeground, 0.7)} />
                  {id ? <Text className="shrink-0 text-[12px] text-muted-foreground">{id}</Text> : null}
                  <Text numberOfLines={1} className="flex-1 min-w-0 text-[13px]">
                    {task.title || 'Untitled task'}
                  </Text>
                </Pressable>
                {below > 0 ? <Text className="shrink-0 text-[12px] text-muted-foreground/70">+{plural(below, 'sub-issue')}</Text> : null}
                <Button label="Restore" icon="refresh" size="sm" variant="outline" disabled={busy} onPress={() => writes.restoreTask.mutate(task)} />
                {canPurge ? (
                  <IconButton icon="trash" iconSize={14} label={`Delete ${task.title || 'Untitled task'} forever`} color={colors.destructive} disabled={busy} onPress={() => requestPurgeTask(task)} />
                ) : null}
              </View>
            )
          })}
        </ScrollView>
      )}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </SafeAreaView>
  )
}
