import { trashedNotice, trashIdentifier } from '@orbit/api/trash'
import { router, useLocalSearchParams } from 'expo-router'
import { View } from 'react-native'

import { Attachments, type ShownFile } from '../../../../../components/tasks/Attachments'
import { LabelPill } from '../../../../../components/tasks/pickers'
import { confirmDeleteForever, Property, TrashBanner, TrashPreview } from '../../../../../components/tasks/TrashPreview'
import { markdownToText } from '../../../../../data/markdown'
import { dueDateLabel, PRIORITY_LABEL, priorityOf, splitInlineFiles } from '../../../../../data/tasks'
import { useTaskTrash, useTaskTrashWrites, useTrashedTask } from '../../../../../hooks/tasks'
import { useOrbit } from '../../../../../orbit-context'
import { usePeople, useProjects } from '../../../../../queries'
import { errorMessage } from '../../../../../session-context'
import { Avatar, Button, PriorityIcon, StatusIcon, Text } from '../../../../../ui'

/** The web's `/tasks-trash/task/:taskId`: a trashed task, read-only, with Restore and Delete forever. */
export default function TrashedTaskScreen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  const { workspace } = useOrbit()
  const canPurge = workspace.permissions.includes('pages.purge')
  const task = useTrashedTask(id)
  const liveProjects = useProjects()
  const { projects: trashedProjects } = useTaskTrash()
  const people = usePeople()
  const writes = useTaskTrashWrites()
  if (task.isError) return <TrashPreview title="" gone="task" />
  if (!task.data || liveProjects.isPending || trashedProjects.isPending) return <TrashPreview title="" loading />
  const record = task.data
  const trashedProject = trashedProjects.data?.find((project) => project.id === record.project_id)
  const project = trashedProject ?? liveProjects.data?.find((item) => item.id === record.project_id)
  const subject = trashIdentifier(record, () => project?.key)
  const busy = writes.restoreTask.isPending || writes.purgeTask.isPending
  const failure = writes.restoreTask.error ?? writes.purgeTask.error
  const nameOf = (userId: string | null) => people.find((person) => person.id === userId)?.name
  const description = splitInlineFiles(record.description)
  const descriptionText = markdownToText(description.text)
  const files: ShownFile[] = description.files.map((file) => ({ key: file.path, ...file }))
  const priority = priorityOf(record)
  const assignees = record.assignee_ids.flatMap((userId) => people.filter((person) => person.id === userId))
  const banner = trashedProject ? (
    <TrashBanner text={`This task’s project “${trashedProject.name}” is in the Trash. Restore the project to get the task back.`}>
      <Button label="Open project" size="sm" variant="outline" onPress={() => router.push(`/tasks/trash/project/${trashedProject.id}`)} />
    </TrashBanner>
  ) : (
    <TrashBanner text={trashedNotice('task', nameOf(record.deleted_by), record.deleted_at ?? record.updated_at)}>
      <Button label="Restore task" icon="refresh" size="sm" variant="outline" disabled={busy} onPress={() => writes.restoreTask.mutate(record, { onSuccess: (restored) => router.replace(`/tasks/${restored.id}`) })} />
      {canPurge ? (
        <Button
          label="Delete forever"
          icon="trash"
          size="sm"
          variant="destructive"
          disabled={busy}
          onPress={() =>
            confirmDeleteForever(record.title || 'Untitled task', 'The task and the sub-issues trashed with it are deleted permanently. They can’t be restored.', () =>
              writes.purgeTask.mutate(record.id, { onSuccess: () => router.navigate('/tasks/trash') }),
            )
          }
        />
      ) : null}
    </TrashBanner>
  )
  return (
    <TrashPreview title={subject ?? 'Task'} banner={banner} error={failure ? errorMessage(failure) : null}>
      <Text selectable className="font-noto-semibold text-[20px] leading-[26px]">
        {record.title || 'Untitled task'}
      </Text>
      <View className="mt-4 gap-1">
        <Property name="Project">
          <Text className="text-[13px]">{project?.name ?? 'Unknown project'}</Text>
        </Property>
        <Property name="Status">
          <StatusIcon status={record.status} size={14} />
          <Text className="text-[13px]">{record.status.name}</Text>
        </Property>
        <Property name="Priority">
          <PriorityIcon priority={priority} />
          <Text className="text-[13px]">{PRIORITY_LABEL[priority]}</Text>
        </Property>
        <Property name="Assignees">
          {assignees.length > 0 ? (
            assignees.map((person) => (
              <View key={person.id} className="flex-row items-center gap-1.5 mr-2">
                <Avatar id={person.id} name={person.name} size={18} />
                <Text className="text-[13px]">{person.name}</Text>
              </View>
            ))
          ) : (
            <Text className="text-[13px] text-muted-foreground">No assignee</Text>
          )}
        </Property>
        <Property name="Labels">
          {record.labels.length > 0 ? record.labels.map((label) => <LabelPill key={label.id} label={label} />) : <Text className="text-[13px] text-muted-foreground">No labels</Text>}
        </Property>
        <Property name="Due">
          {record.due_at ? <Text className="text-[13px]">{dueDateLabel(record.due_at, record.due_start_at)}</Text> : <Text className="text-[13px] text-muted-foreground">No due date</Text>}
        </Property>
      </View>
      {descriptionText ? (
        <Text selectable className="text-[16px] leading-[24px] mt-[19px]">
          {descriptionText}
        </Text>
      ) : null}
      <Attachments files={files} />
    </TrashPreview>
  )
}
