/** The main-column sections of the task page under the description: Duplicate banner, Sub-issues, Relations. */
import { router } from 'expo-router'
import { type ReactNode, useState } from 'react'
import { Pressable, View } from 'react-native'
import { TextInput } from '../../ui/scaled'

import type { StatusRecord, TaskRecord } from '../../data/orbit'
import type { TaskRelationRecord } from '../../data/task-api'
import { defaultStatusOf, groupRelations, identifier, isClosed, refIdentifier } from '../../data/tasks'
import { useProjects } from '../../queries'
import { useTheme } from '../../theme'
import { Button, Icon, IconButton, StatusIcon, Text } from '../../ui'

const openTask = (taskId: string) => router.push(`/tasks/${taskId}`)

function SectionTitle({ title, count, children }: { title: string; count: ReactNode; children?: ReactNode }) {
  return (
    <View className="min-h-7 flex-row items-center gap-1.5 mb-1">
      <Text className="font-noto-semibold text-[12px] text-muted-foreground">{title}</Text>
      {typeof count === 'number' ? <Text className="text-[12px] text-muted-foreground/60">{count}</Text> : count}
      <View className="flex-1" />
      {children}
    </View>
  )
}

/** One task line: status, identifier and title; a tap opens the task. */
function TaskLine({ status, taskId, id, title, label, trailing }: { status: StatusRecord | undefined; taskId: string; id: string; title: string; label?: string; trailing?: ReactNode }) {
  return (
    <View className="min-h-9 flex-row items-center gap-2">
      {label !== undefined ? (
        <Text numberOfLines={1} className="text-[12px] w-18 text-muted-foreground/70">
          {label}
        </Text>
      ) : null}
      <Pressable accessibilityRole="link" accessibilityLabel={`${id} ${title}`} onPress={() => openTask(taskId)} className="flex-1 flex-row items-center gap-2 min-w-0">
        <StatusIcon status={status} size={14} />
        <Text className="text-[12px] text-muted-foreground">{id}</Text>
        <Text numberOfLines={1} className="text-[13px] flex-1">
          {title || 'Untitled'}
        </Text>
      </Pressable>
      {trailing}
    </View>
  )
}

/** The web's strip above the title while the task is a duplicate: the canonical task (opens it) and Unmark. */
export function DuplicateBanner({ task, status, pending, onUnmark }: { task: TaskRecord; status: StatusRecord | undefined; pending: boolean; onUnmark: () => void }) {
  const target = task.duplicate_of
  if (!target) return null
  return (
    <View accessibilityLabel={`Duplicate of ${refIdentifier(target)}`} className="min-h-10 flex-row items-center gap-2 pb-2 mb-1.5 border-b border-border">
      <StatusIcon status={status} size={14} />
      <Text className="text-[13px] text-muted-foreground">Duplicate of</Text>
      <Pressable accessibilityRole="link" onPress={() => openTask(target.id)} className="flex-1 flex-row gap-1.5 min-w-0">
        <Text className="text-[13px] shrink-0 text-muted-foreground">{refIdentifier(target)}</Text>
        <Text numberOfLines={1} className="font-noto-medium text-[13px] shrink">
          {target.title || 'Untitled'}
        </Text>
      </Pressable>
      <Button label="Unmark" variant="ghost" size="xs" disabled={pending} onPress={onUnmark} />
    </View>
  )
}

/**
 * Sub-issues: the closed count, the direct sub-issues, and the inline composer of the web (a title, Enter creates it
 * with the parent's project, default status, priority and assignees). The composer stays for the next title.
 */
export function SubIssuesSection({
  tasks,
  statuses,
  composing,
  creating,
  failed,
  onCompose,
  onAddExisting,
  onCreate,
  onCloseComposer,
  projectId,
}: {
  tasks: readonly TaskRecord[]
  statuses: readonly StatusRecord[]
  composing: boolean
  creating: boolean
  failed: boolean
  onCompose: () => void
  onAddExisting: () => void
  onCreate: (title: string) => void
  onCloseComposer: () => void
  projectId: string
}) {
  const { colors, scheme } = useTheme()
  const projects = useProjects()
  const [title, setTitle] = useState('')
  if (tasks.length === 0 && !composing) return null
  const statusOf = (statusId: string) => statuses.find((status) => status.id === statusId)
  const closed = tasks.filter((task) => isClosed(statusOf(task.status_id))).length
  const submit = () => {
    const text = title.trim()
    if (!text) return
    setTitle('')
    onCreate(text)
  }
  return (
    <View className="border-t border-border pt-3">
      <SectionTitle
        title="Sub-issues"
        count={
          <View className="h-5.5 flex-row items-center gap-1 px-[7px] rounded-full border border-border">
            <Icon name="circle" size={12} color={colors.mutedForeground} />
            <Text className="text-[12px] text-muted-foreground">{`${closed}/${tasks.length}`}</Text>
          </View>
        }
      >
        <Button label="Add existing" variant="ghost" size="xs" muted onPress={onAddExisting} />
        <IconButton icon="plus" label="Add sub-issue" size={24} iconSize={14} color={colors.mutedForeground} onPress={onCompose} />
      </SectionTitle>
      {tasks.map((task) => (
        <TaskLine key={task.id} taskId={task.id} status={statusOf(task.status_id)} id={identifier(task, projects.data ?? [])} title={task.title} />
      ))}
      {composing ? (
        <View className="min-h-9 flex-row items-center gap-2 px-2 -mx-2 rounded-md bg-muted/50">
          <StatusIcon status={defaultStatusOf(statuses, projectId)} size={14} />
          <TextInput
            autoFocus
            value={title}
            onChangeText={setTitle}
            onSubmitEditing={submit}
            submitBehavior="submit"
            returnKeyType="done"
            placeholder="Sub-issue title"
            accessibilityLabel="Sub-issue title"
            placeholderTextColor={colors.mutedForeground}
            selectionColor={colors.primary}
            keyboardAppearance={scheme}
            className="flex-1 font-noto text-[16px] leading-[22px] py-1.5 text-foreground"
          />
          {creating ? <Text className="text-[12px] text-muted-foreground">Creating…</Text> : null}
          <IconButton icon="close" label="Close the composer" size={24} iconSize={14} color={colors.mutedForeground} onPress={onCloseComposer} />
        </View>
      ) : null}
      {failed ? <Text className="text-[12px] text-destructive mt-1">Couldn’t create the sub-issue.</Text> : null}
    </View>
  )
}

/** Relations in the web's groups (Blocked by, Blocks, Related …), each with a remove button. */
export function RelationsSection({ relations, statuses, removingId, onRemove }: { relations: readonly TaskRelationRecord[]; statuses: readonly StatusRecord[]; removingId: string | undefined; onRemove: (relationId: string) => void }) {
  const { colors } = useTheme()
  const projects = useProjects()
  const groups = groupRelations(relations)
  if (groups.length === 0) return null
  const count = groups.reduce((total, group) => total + group.relations.length, 0)
  return (
    <View className="border-t border-border pt-3">
      <SectionTitle title="Relations" count={count} />
      {groups.map((group) =>
        group.relations.map((relation, index) => {
          const id = identifier(relation.task, projects.data ?? [])
          return (
            <TaskLine
              key={relation.id}
              taskId={relation.task.id}
              label={index === 0 ? group.label : ''}
              status={statuses.find((status) => status.id === relation.task.status_id)}
              id={id}
              title={relation.task.title}
              trailing={<IconButton icon="close" label={`Remove relation to ${id}`} size={24} iconSize={12} color={colors.mutedForeground} disabled={removingId === relation.id} onPress={() => onRemove(relation.id)} />}
            />
          )
        }),
      )}
    </View>
  )
}
