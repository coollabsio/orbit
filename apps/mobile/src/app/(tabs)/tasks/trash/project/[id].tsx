import { trashedNotice } from '@orbit/api/trash'
import { router, useLocalSearchParams } from 'expo-router'
import { Pressable, View } from 'react-native'

import { confirmDeleteForever, TrashBanner, TrashPreview } from '../../../../../components/tasks/TrashPreview'
import { useTaskTrashWrites, useTrashedProject } from '../../../../../hooks/tasks'
import { useOrbit } from '../../../../../orbit-context'
import { usePeople } from '../../../../../queries'
import { errorMessage } from '../../../../../session-context'
import { Button, StatusIcon, Text } from '../../../../../ui'

/** The web's `/tasks-trash/project/:projectId`: a trashed project and its tasks, read-only. */
export default function TrashedProjectScreen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  const { workspace } = useOrbit()
  const canPurge = workspace.permissions.includes('pages.purge')
  const { project, tasks } = useTrashedProject(id)
  const people = usePeople()
  const writes = useTaskTrashWrites()
  if (project.isError || tasks.isError) return <TrashPreview title="" gone="project" />
  if (!project.data || !tasks.data) return <TrashPreview title="" loading />
  const record = project.data
  const statuses = new Map(record.statuses.map((status) => [status.id, status]))
  const busy = writes.restoreProject.isPending || writes.purgeProject.isPending
  const failure = writes.restoreProject.error ?? writes.purgeProject.error
  const deletedBy = people.find((person) => person.id === record.deleted_by)?.name
  return (
    <TrashPreview
      title={record.name}
      error={failure ? errorMessage(failure) : null}
      banner={
        <TrashBanner text={trashedNotice('project', deletedBy, record.deleted_at ?? record.updated_at)}>
          <Button label="Restore project" icon="refresh" size="sm" variant="outline" disabled={busy} onPress={() => writes.restoreProject.mutate(record, { onSuccess: () => router.replace({ pathname: '/tasks', params: { project: record.id } }) })} />
          {canPurge ? (
            <Button
              label="Delete forever"
              icon="trash"
              size="sm"
              variant="destructive"
              disabled={busy}
              onPress={() =>
                confirmDeleteForever(record.name, 'The project and all its tasks are deleted permanently. They can’t be restored.', () =>
                  writes.purgeProject.mutate(record.id, { onSuccess: () => router.navigate('/tasks/trash') }),
                )
              }
            />
          ) : null}
        </TrashBanner>
      }
    >
      <Text selectable className="font-noto-semibold text-[20px] leading-[26px]">
        {record.name}
      </Text>
      <Text className="mt-1 text-[13px] text-muted-foreground">
        {record.key} · {tasks.data.length === 1 ? '1 task' : `${tasks.data.length} tasks`}
      </Text>
      {tasks.data.length === 0 ? (
        <Text className="mt-6 text-[13px] text-muted-foreground">This project has no tasks.</Text>
      ) : (
        <View className="mt-4 border-t border-border">
          {tasks.data.map((task) => (
            <Pressable
              key={task.id}
              accessibilityRole="link"
              onPress={() => router.push(`/tasks/trash/task/${task.id}`)}
              className="min-h-10 flex-row items-center gap-2.5 border-b border-border px-1 py-1.5 active:bg-muted"
            >
              <Text className="w-16 shrink-0 text-[12px] text-muted-foreground">
                {record.key}-{task.number}
              </Text>
              <StatusIcon status={statuses.get(task.status_id)} size={14} />
              <Text numberOfLines={1} className="flex-1 min-w-0 text-[13px]">
                {task.title || 'Untitled task'}
              </Text>
            </Pressable>
          ))}
        </View>
      )}
    </TrashPreview>
  )
}
