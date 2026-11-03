import { useMutation, useQueryClient } from '@tanstack/react-query'
import { router } from 'expo-router'
import { type ReactNode, useState } from 'react'
import { Alert, KeyboardAvoidingView, Modal, Platform, Pressable, ScrollView, useWindowDimensions, View } from 'react-native'
import { TextInput } from '../ui/scaled'

import { keys } from '../data/keys'
import * as api from '../data/orbit'
import * as taskApi from '../data/task-api'
import {
  attachmentPath,
  bodyWithFiles,
  defaultStatusOf,
  type DueRange,
  dueDateLabel,
  identifier,
  isImageType,
  matchingLabelIds,
  type PickedFile,
  PRIORITY_LABEL,
  type Priority,
  projectStatuses,
  RELATION_OPTIONS,
  type RelationKind,
} from '../data/tasks'
import { useOrbit } from '../orbit-context'
import { useLabels, usePeople, useProjects, useStatuses } from '../queries'
import { errorMessage } from '../session-context'
import { alpha, cssColors, useTheme } from '../theme'
import { Avatar, Button, Icon, type IconName, IconButton, PriorityIcon, StatusIcon, Text } from '../ui'
import { type FileSource, pickFiles } from './tasks/files'
import { Dot, DueDateChoices, LabelChoices, MenuChoices, PeopleChoices, PriorityChoices, StatusChoices, TaskChoices } from './tasks/pickers'
import { Sheet, SheetList, SheetRow, useSheet } from './tasks/Sheet'

/** Relations a new task can have: a duplicate needs a canonical task, which it cannot be marked as before it exists. */
type PendingKind = Exclude<RelationKind, 'duplicate'>
const PENDING_RELATIONS = RELATION_OPTIONS.filter((option): option is { kind: PendingKind; label: string } => option.kind !== 'duplicate')
const RELATION_LABEL: Record<PendingKind, string> = { blocks: 'Blocks', blocked_by: 'Blocked by', related: 'Related to' }

type DialogSheet =
  | { kind: 'project' | 'status' | 'priority' | 'assignees' | 'labels' | 'due' | 'parent-menu' | 'relation-menu' | 'attach' }
  | { kind: 'pick-task'; purpose: 'parent' | PendingKind }

function sheetTitle(sheet: DialogSheet | null): string {
  if (!sheet) return ''
  switch (sheet.kind) {
    case 'project':
      return 'Project'
    case 'status':
      return 'Status'
    case 'priority':
      return 'Priority'
    case 'assignees':
      return 'Assignees'
    case 'labels':
      return 'Labels'
    case 'due':
      return 'Due date'
    case 'parent-menu':
      return 'Parent'
    case 'relation-menu':
      return 'Add relation'
    case 'attach':
      return 'Attach'
    case 'pick-task':
      return sheet.purpose === 'parent' ? 'New task: Parent…' : `New task: ${RELATION_LABEL[sheet.purpose]}…`
  }
}

export interface NewTaskDefaults {
  projectId?: string
  statusId?: string
}

/**
 * The web's New task dialog: a card 14% from the top over a faint scrim, with property chips and a footer. Mounted
 * while open, so each opening starts from `defaults`. Relations and files wait here and are added once the task
 * exists; the files go into the description, after its text, as on the web.
 */
export function NewTaskDialog({ defaults, onClose }: { defaults: NewTaskDefaults; onClose: () => void }) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const { colors, scheme } = useTheme()
  const { height } = useWindowDimensions()
  const projects = useProjects()
  const statuses = useStatuses()
  const labels = useLabels()
  const people = usePeople()
  const sheets = useSheet<DialogSheet>()
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [projectId, setProjectId] = useState<string | null>(defaults.projectId ?? null)
  const [statusId, setStatusId] = useState<string | null>(defaults.statusId ?? null)
  const [priority, setPriority] = useState<Priority>('none')
  const [assigneeIds, setAssigneeIds] = useState<string[]>([])
  const [chosenLabelIds, setChosenLabelIds] = useState<string[]>([])
  const [due, setDue] = useState<DueRange | null>(null)
  const [parent, setParent] = useState<api.TaskRecord | null>(null)
  const [relations, setRelations] = useState<readonly { kind: PendingKind; task: api.TaskRecord }[]>([])
  const [files, setFiles] = useState<readonly PickedFile[]>([])
  const [createMore, setCreateMore] = useState(false)
  const [created, setCreated] = useState<api.TaskRecord | null>(null)

  const project = projects.data?.find((item) => item.id === projectId) ?? projects.data?.[0]
  const own = project && statuses.data ? projectStatuses(statuses.data, project.id).filter((item) => item.category !== 'duplicate') : []
  const status = own.find((item) => item.id === statusId) ?? (project && statuses.data ? defaultStatusOf(statuses.data, project.id) : undefined)
  // labels follow by name into another project, like a task that moves
  const labelIds = project && labels.data ? matchingLabelIds(labels.data, chosenLabelIds, project.id) : chosenLabelIds
  const chosenLabels = (labels.data ?? []).filter((label) => labelIds.includes(label.id)).sort((a, b) => a.name.localeCompare(b.name))
  const assignees = people.filter((person) => assigneeIds.includes(person.id))

  const create = useMutation({
    mutationFn: async () => {
      if (!project || !status) throw new Error('Choose a project first.')
      const task = await taskApi.createTaskWith(
        client,
        workspaceId,
        taskApi.newTaskBody({ title, description: files.length > 0 ? '' : description, projectId: project.id, statusId: status.id, priority, assigneeIds, labelIds, dueAt: due?.end ?? null, dueStartAt: due?.start ?? null, parentId: parent?.id ?? null }),
      )
      const problems: string[] = []
      if (relations.length > 0) {
        const results = await Promise.allSettled(relations.map((relation) => taskApi.addRelation(client, workspaceId, task.id, { type: relation.kind, task_id: relation.task.id })))
        if (results.some((result) => result.status === 'rejected')) problems.push('some relations were not added')
      }
      if (files.length > 0) {
        const uploaded = await Promise.allSettled(files.map((file) => taskApi.uploadInline(client, workspaceId, task.id, file)))
        const links = uploaded.flatMap((result, index) => (result.status === 'fulfilled' ? [{ name: files[index]!.name, path: attachmentPath(result.value), image: isImageType(result.value.media_type) }] : []))
        if (links.length < files.length) problems.push('some files were not added')
        const body = bodyWithFiles(description, links)
        if (body) await api.changeTask(client, workspaceId, task.id, { description: body, expected_version: task.version }).catch(() => problems.push('the description was not saved'))
      }
      return { task, problems }
    },
    onSuccess: ({ task, problems }) => {
      void queryClient.invalidateQueries({ queryKey: keys.tasks(workspaceId) })
      if (problems.length > 0) Alert.alert('Task created', `The task was created, but ${problems.join(' and ')}.`)
      if (createMore) {
        // the properties stay for the next task of the batch
        setCreated(task)
        setTitle('')
        setDescription('')
        setFiles([])
        setRelations([])
        return
      }
      onClose()
      router.push(`/tasks/${task.id}`)
    },
  })

  const canCreate = title.trim().length > 0 && project !== undefined && status !== undefined && !create.isPending
  const sheet = sheets.sheet
  const attach = async (source: FileSource) => {
    try {
      const picked = await pickFiles(source)
      setFiles((list) => [...list, ...picked])
    } catch (error) {
      Alert.alert('Could not open the picker', errorMessage(error))
    } finally {
      sheets.close()
    }
  }

  return (
    <Modal visible transparent animationType="fade" onRequestClose={onClose}>
      <Pressable accessibilityLabel="Close" onPress={onClose} className="absolute inset-0 bg-black/10" />
      <KeyboardAvoidingView behavior={Platform.OS === 'ios' ? 'padding' : undefined} className="flex-1" pointerEvents="box-none">
        <View
          accessibilityViewIsModal
          className="mx-4 rounded-xl bg-popover overflow-hidden" style={{ marginTop: Math.round(height * 0.14), maxHeight: height * 0.8, boxShadow: `0 0 0 1px ${alpha(colors.foreground, 0.1)}` }}
        >
          <ScrollView contentContainerClassName="p-4 pb-3 gap-3" keyboardShouldPersistTaps="handled">
            <View className="flex-row items-center gap-1.5 pr-8">
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={`Project: ${project?.name ?? 'none'}`}
                onPress={() => sheets.open({ kind: 'project' })}
                className="h-6 flex-row items-center gap-1 px-2 rounded-md border border-input bg-input/30" style={{ boxShadow: `0 2px 0 ${alpha(scheme === 'dark' ? cssColors.white : cssColors.black, scheme === 'dark' ? 0.08 : 0.22)}` }}
              >
                {project ? <Dot color={project.color} size={6} /> : null}
                <Text numberOfLines={1} className="text-[12px] leading-[16px] max-w-42.5">
                  {project?.name ?? 'No project'}
                </Text>
              </Pressable>
              <Icon name="chevronRight" size={12} color={alpha(colors.mutedForeground, 0.7)} />
              <Text className="text-[12px] leading-[16px] text-muted-foreground">New task</Text>
            </View>
            <View className="absolute top-2 right-2">
              <IconButton icon="close" label="Close" onPress={onClose} />
            </View>

            <View className="gap-1.5">
              <TextInput
                accessibilityLabel="Task title"
                placeholder="Task title"
                placeholderTextColor={colors.mutedForeground}
                selectionColor={colors.primary}
                keyboardAppearance={scheme}
                autoFocus
                value={title}
                onChangeText={setTitle}
                className="font-noto-medium text-[16px] leading-[25px] p-0 text-foreground"
              />
              <TextInput
                accessibilityLabel="Description"
                placeholder="Add description…"
                placeholderTextColor={colors.mutedForeground}
                selectionColor={colors.primary}
                keyboardAppearance={scheme}
                multiline
                value={description}
                onChangeText={setDescription}
                className="font-noto text-[16px] leading-[24px] min-h-20 p-0 pt-0.5 text-foreground" style={{ textAlignVertical: 'top' }}
              />
              {files.length > 0 || relations.length > 0 ? (
                <View className="flex-row flex-wrap gap-1.5">
                  {files.map((file, index) => (
                    <Pending key={`${file.uri}-${index}`} label={`Remove ${file.name}`} onRemove={() => setFiles((list) => list.filter((_, at) => at !== index))}>
                      <Icon name="attach" size={12} color={colors.mutedForeground} />
                      <Text numberOfLines={1} className="text-[12px] max-w-45">
                        {file.name}
                      </Text>
                    </Pending>
                  ))}
                  {relations.map((relation) => (
                    <Pending
                      key={`${relation.kind}:${relation.task.id}`}
                      label={`Remove relation to ${identifier(relation.task, projects.data ?? [])}`}
                      onRemove={() => setRelations((list) => list.filter((item) => item !== relation))}
                    >
                      <Text className="text-[12px] text-muted-foreground">{RELATION_LABEL[relation.kind]}</Text>
                      <Text className="text-[12px] text-muted-foreground">{identifier(relation.task, projects.data ?? [])}</Text>
                      <Text numberOfLines={1} className="text-[12px] max-w-35">
                        {relation.task.title || 'Untitled'}
                      </Text>
                    </Pending>
                  ))}
                </View>
              ) : null}
              <View className="flex-row -ml-2">
                <Quiet icon="attach" label="Attach" onPress={() => sheets.open({ kind: 'attach' })} />
                <Quiet icon="relation" label="Add relation" onPress={() => sheets.open({ kind: 'relation-menu' })} />
              </View>
            </View>

            <View className="flex-row flex-wrap gap-1.5">
              <Chip label={`Status: ${status?.name ?? 'none'}`} onPress={() => sheets.open({ kind: 'status' })}>
                <StatusIcon status={status} size={14} />
                <ChipText>{status?.name ?? 'No status'}</ChipText>
              </Chip>
              <Chip label={`Priority: ${PRIORITY_LABEL[priority]}`} onPress={() => sheets.open({ kind: 'priority' })}>
                <PriorityIcon priority={priority} size={14} />
                <ChipText>{priority === 'none' ? 'Priority' : PRIORITY_LABEL[priority]}</ChipText>
              </Chip>
              <Chip label="Assignees" onPress={() => sheets.open({ kind: 'assignees' })}>
                {assignees.length === 0 ? <Icon name="user" size={14} color={colors.mutedForeground} /> : <Avatar id={assignees[0]!.id} name={assignees[0]!.name} size={14} />}
                <ChipText>{assignees.length === 0 ? 'Assignee' : assignees.map((person) => person.name).join(', ')}</ChipText>
              </Chip>
              <Chip label={chosenLabels.length > 0 ? `Labels: ${chosenLabels.map((label) => label.name).join(', ')}` : 'Labels'} onPress={project ? () => sheets.open({ kind: 'labels' }) : undefined}>
                {chosenLabels.length > 0 ? (
                  <View className="flex-row gap-0.5">
                    {chosenLabels.slice(0, 3).map((label) => (
                      <Dot key={label.id} color={label.color} />
                    ))}
                  </View>
                ) : (
                  <Icon name="tag" size={14} color={colors.mutedForeground} />
                )}
                <ChipText>{chosenLabels.length > 0 ? chosenLabels.map((label) => label.name).join(', ') : 'Labels'}</ChipText>
              </Chip>
              <Chip label="Due date" onPress={() => sheets.open({ kind: 'due' })}>
                <Icon name="calendar" size={14} color={colors.foreground} />
                <ChipText>{due ? dueDateLabel(due.end, due.start) : 'Due date'}</ChipText>
              </Chip>
              <Chip
                label={parent ? `Parent: ${identifier(parent, projects.data ?? [])} ${parent.title}` : 'Parent'}
                onPress={() => sheets.open(parent ? { kind: 'parent-menu' } : { kind: 'pick-task', purpose: 'parent' })}
              >
                <Icon name="subIssue" size={14} color={colors.mutedForeground} />
                {parent ? <Text className="text-[12.8px] leading-[18px] text-muted-foreground">{identifier(parent, projects.data ?? [])}</Text> : null}
                <ChipText>{parent ? parent.title || 'Untitled' : 'Parent'}</ChipText>
              </Chip>
            </View>
            {create.isError ? <Text className="text-[12px] text-destructive">{errorMessage(create.error)}</Text> : null}
          </ScrollView>

          <View className="flex-row items-center justify-center gap-3 px-4 pt-2.5 pb-4.5 border-t border-border bg-muted/50">
            {created ? (
              <Pressable accessibilityRole="link" onPress={() => {
                  onClose()
                  router.push(`/tasks/${created.id}`)
                }} className="shrink">
                <Text numberOfLines={1} className="text-[12px] text-muted-foreground">
                  Created <Text className="font-noto-medium text-[12px] text-foreground">{identifier(created, projects.data ?? [])}</Text>
                </Text>
              </Pressable>
            ) : null}
            <Pressable accessibilityRole="switch" accessibilityState={{ checked: createMore }} onPress={() => setCreateMore((value) => !value)} className="flex-row items-center gap-2">
              <View className={`w-6 h-3.5 rounded-full p-px ${createMore ? 'items-end bg-primary' : 'items-start bg-foreground/12'}`}>
                <View className={`size-3 rounded-full ${createMore ? 'bg-primary-foreground' : 'bg-foreground'}`} />
              </View>
              <Text className="text-[12px] leading-[16px] text-muted-foreground">Create more</Text>
            </Pressable>
            <Button label={files.length > 0 && create.isPending ? 'Attaching…' : 'Create task'} loading={create.isPending} disabled={!canCreate} onPress={() => create.mutate()} />
          </View>
        </View>
      </KeyboardAvoidingView>

      <Sheet visible={sheets.visible} title={sheetTitle(sheet)} onClose={sheets.close}>
        {sheet?.kind === 'project' ? (
          <SheetList>
            {(projects.data ?? []).map((item) => (
              <SheetRow
                key={item.id}
                label={item.name}
                subtitle={item.key}
                leading={<Dot color={item.color} />}
                checked={item.id === project?.id}
                onPress={() => {
                  sheets.close()
                  setProjectId(item.id)
                  // a status belongs to one project: the new project starts at its default
                  setStatusId(null)
                }}
              />
            ))}
          </SheetList>
        ) : null}
        {sheet?.kind === 'status' ? (
          <StatusChoices
            statuses={own}
            value={status?.id}
            onSelect={(id) => {
              sheets.close()
              setStatusId(id)
            }}
          />
        ) : null}
        {sheet?.kind === 'priority' ? (
          <PriorityChoices
            value={priority}
            onSelect={(next) => {
              sheets.close()
              setPriority(next)
            }}
          />
        ) : null}
        {sheet?.kind === 'assignees' ? <PeopleChoices people={people} value={assigneeIds} onChange={setAssigneeIds} /> : null}
        {sheet?.kind === 'labels' && project ? <LabelChoices projectId={project.id} value={labelIds} onChange={setChosenLabelIds} /> : null}
        {sheet?.kind === 'due' ? (
          <DueDateChoices
            dueAt={due?.end}
            onChange={(next) => {
              sheets.close()
              setDue(next)
            }}
          />
        ) : null}
        {sheet?.kind === 'parent-menu' ? (
          <MenuChoices
            items={[
              { key: 'change', label: 'Change parent…', icon: 'subIssue' },
              { key: 'remove', label: 'Remove parent', icon: 'close' },
            ]}
            onSelect={(key) => {
              if (key === 'change') return sheets.open({ kind: 'pick-task', purpose: 'parent' })
              sheets.close()
              setParent(null)
            }}
          />
        ) : null}
        {sheet?.kind === 'relation-menu' ? (
          <MenuChoices items={PENDING_RELATIONS.map((option) => ({ key: option.kind, label: option.label }))} onSelect={(kind) => sheets.open({ kind: 'pick-task', purpose: kind })} />
        ) : null}
        {sheet?.kind === 'pick-task' ? (
          <TaskChoices
            key={sheet.purpose}
            excludeIds={sheet.purpose === 'parent' ? [] : relations.map((relation) => relation.task.id)}
            onSelect={(target) => {
              const purpose = sheet.purpose
              sheets.close()
              if (purpose === 'parent') setParent(target)
              else setRelations((list) => [...list, { kind: purpose, task: target }])
            }}
          />
        ) : null}
        {sheet?.kind === 'attach' ? (
          <MenuChoices
            items={[
              { key: 'photos', label: 'Photo library', icon: 'sticker' },
              { key: 'files', label: 'Choose file', icon: 'attach' },
            ]}
            onSelect={(source) => void attach(source)}
          />
        ) : null}
      </Sheet>
    </Modal>
  )
}

/** The web's outline `sm` button: a 28px chip with the 2px edge. */
function Chip({ label, onPress, children }: { label: string; onPress?: () => void; children: ReactNode }) {
  const { scheme } = useTheme()
  const edge = alpha(scheme === 'dark' ? cssColors.white : cssColors.black, scheme === 'dark' ? 0.08 : 0.22)
  return (
    <Pressable
      accessibilityRole={onPress ? 'button' : undefined}
      accessibilityLabel={label}
      disabled={!onPress}
      onPress={onPress}
      className="h-7 flex-row items-center gap-1 rounded-md border border-input bg-input/30 px-2.5 active:bg-input/50"
      style={({ pressed }) => ({ boxShadow: pressed ? undefined : `0 2px 0 ${edge}`, transform: [{ translateY: pressed ? 2 : 0 }] })}
    >
      {children}
    </Pressable>
  )
}

function ChipText({ children }: { children: ReactNode }) {
  return (
    <Text numberOfLines={1} className="text-[12.8px] leading-[18px] max-w-50">
      {children}
    </Text>
  )
}

function Quiet({ icon, label, onPress }: { icon: IconName; label: string; onPress: () => void }) {
  const { colors } = useTheme()
  const color = alpha(colors.mutedForeground, 0.7)
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      onPress={onPress}
      className="h-8 flex-row items-center gap-1.5 px-2.5 rounded-lg active:bg-muted"
    >
      <Icon name={icon} size={14} color={color} />
      <Text className="font-noto-medium text-[12px] leading-[16px] text-muted-foreground/70">{label}</Text>
    </Pressable>
  )
}

/** A file or relation waiting for the task: a small outline chip with a remove button. */
function Pending({ label, onRemove, children }: { label: string; onRemove: () => void; children: ReactNode }) {
  const { colors } = useTheme()
  return (
    <View className="h-6.5 max-w-full flex-row items-center gap-1 pl-2 pr-0.5 rounded-md border border-border">
      {children}
      <IconButton icon="close" label={label} size={20} iconSize={12} color={colors.mutedForeground} onPress={onRemove} />
    </View>
  )
}
