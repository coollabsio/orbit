import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { openURL } from 'expo-linking'
import { router, useLocalSearchParams } from 'expo-router'
import { type ReactNode, useMemo, useState } from 'react'
import { Alert, KeyboardAvoidingView, Platform, Pressable, RefreshControl, ScrollView, View } from 'react-native'
import { TextInput } from '../../../ui/scaled'

import { Attachments, type ShownFile } from '../../../components/tasks/Attachments'
import { type FileSource, pickFiles } from '../../../components/tasks/files'
import {
  DueDateChoices,
  LabelChoices,
  LabelPill,
  MenuChoices,
  PeopleChoices,
  PriorityChoices,
  SourceForm,
  StatusChoices,
  TaskChoices,
} from '../../../components/tasks/pickers'
import { DuplicateBanner, RelationsSection, SubIssuesSection } from '../../../components/tasks/sections'
import { Sheet, useSheet } from '../../../components/tasks/Sheet'
import { activityRows, buildFeed, type ActivityRow, type CommentThread, foldRun } from '../../../data/activity'
import { relativeAgo } from '../../../data/format'
import { keys } from '../../../data/keys'
import { markdownToText } from '../../../data/markdown'
import * as api from '../../../data/orbit'
import * as taskApi from '../../../data/task-api'
import {
  attachmentPath,
  bodyWithFiles,
  dueDateLabel,
  identifier,
  isImageType,
  PRIORITY_LABEL,
  priorityOf,
  projectStatuses,
  refIdentifier,
  RELATION_OPTIONS,
  relatedTaskIds,
  type RelationKind,
  relationPickerTitle,
  sourceHost,
  splitInlineFiles,
  subIssueBody,
} from '../../../data/tasks'
import { useAdoptSubIssue, useCommentAttachments, useCreateTask, useRelationWrites, useSubIssues, useTaskAttachments, useTaskRelations, useTaskWrite } from '../../../hooks/tasks'
import { useOrbit } from '../../../orbit-context'
import { useLabels, usePeople, useProjects, useStatuses } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { alpha, useTheme } from '../../../theme'
import { Avatar, Button, ErrorView, Icon, type IconName, IconButton, LoadingView, PaneHeader, PriorityIcon, SafeAreaView, StatusIcon, Text } from '../../../ui'

/** The one sheet of the page and what it shows. */
type TaskSheet =
  | { kind: 'status' | 'priority' | 'assignees' | 'labels' | 'due' | 'source' | 'parent-menu' | 'relation-menu' | 'attach' }
  | { kind: 'pick-task'; purpose: 'parent' | 'sub-issue' | RelationKind }

function sheetTitle(sheet: TaskSheet | null, subject: string): string {
  if (!sheet) return ''
  switch (sheet.kind) {
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
    case 'source':
      return 'Source'
    case 'parent-menu':
      return 'Parent'
    case 'relation-menu':
      return 'Add relation'
    case 'attach':
      return 'Attach'
    case 'pick-task':
      if (sheet.purpose === 'parent') return `Set parent of ${subject}…`
      if (sheet.purpose === 'sub-issue') return `Add a sub-issue to ${subject}…`
      return relationPickerTitle(sheet.purpose, subject)
  }
}

/** A file of the comment being written: uploading (no `path` yet) or uploaded. */
type DraftFile = ShownFile & { image: boolean }

let draftFileCount = 0

export default function TaskScreen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  const { client, workspaceId, user } = useOrbit()
  const queryClient = useQueryClient()
  const { colors } = useTheme()
  const projects = useProjects()
  const statuses = useStatuses()
  const labels = useLabels()
  const people = usePeople()
  const sheets = useSheet<TaskSheet>()
  const [draft, setDraft] = useState('')
  const [draftFiles, setDraftFiles] = useState<readonly DraftFile[]>([])
  const [unfolded, setUnfolded] = useState<readonly string[]>([])
  const [composingSubIssue, setComposingSubIssue] = useState(false)

  const task = useQuery({
    queryKey: keys.task(workspaceId, id),
    queryFn: () => api.task(client, workspaceId, id),
    // the list row the user tapped, until the full task arrives
    placeholderData: () => queryClient.getQueryData<api.TaskRecord[]>(keys.tasks(workspaceId))?.find((item) => item.id === id),
  })
  const comments = useQuery({ queryKey: keys.comments(workspaceId, id), queryFn: () => api.comments(client, workspaceId, id) })
  const activity = useQuery({ queryKey: keys.taskActivity(workspaceId, id), queryFn: () => api.taskActivity(client, workspaceId, id) })
  const relations = useTaskRelations(id)
  const subIssues = useSubIssues(id)
  const attachments = useTaskAttachments(id)
  const commentFiles = useCommentAttachments(id, comments.data ?? [])
  const write = useTaskWrite(id)
  const relationWrites = useRelationWrites(id)
  const createSubIssue = useCreateTask()
  const adopt = useAdoptSubIssue(id)

  const sendComment = useMutation({
    mutationFn: (body: string) => api.addComment(client, workspaceId, id, body),
    onSuccess: (comment) => {
      setDraft('')
      setDraftFiles([])
      queryClient.setQueryData<api.CommentRecord[]>(keys.comments(workspaceId, id), (list) => (list ? [...list, comment] : [comment]))
    },
    onError: (error) => Alert.alert('Could not add the comment', errorMessage(error)),
  })

  const trash = useMutation({
    mutationFn: (current: api.TaskRecord) => api.trashTask(client, workspaceId, current),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.tasks(workspaceId) })
      close()
    },
    onError: (error) => Alert.alert('Task deletion failed', errorMessage(error)),
  })

  const close = () => (router.canGoBack() ? router.back() : router.navigate('/tasks'))
  const nameOf = (userId: string) => people.find((person) => person.id === userId)?.name ?? 'Unknown'
  const current = task.data
  const project = projects.data?.find((item) => item.id === current?.project_id)

  const feed = useMemo(() => {
    if (!activity.data || !comments.data) return []
    const known = queryClient.getQueryData<api.TaskRecord[]>(keys.tasks(workspaceId)) ?? []
    const rows = activityRows(activity.data, {
      statuses: statuses.data ?? [],
      people,
      labels: labels.data ?? [],
      projects: projects.data ?? [],
      identifierOf: (taskId) => {
        const other = known.find((item) => item.id === taskId)
        return other ? identifier(other, projects.data ?? []) : 'another task'
      },
    })
    return buildFeed(rows, comments.data)
  }, [activity.data, comments.data, labels.data, people, projects.data, queryClient, statuses.data, workspaceId])

  /** Uploads each picked file at once, as the web composer does; the comment links them when it is sent. */
  const attach = async (source: FileSource) => {
    let picked: Awaited<ReturnType<typeof pickFiles>>
    try {
      picked = await pickFiles(source)
    } catch (error) {
      sheets.close()
      return Alert.alert('Could not open the picker', errorMessage(error))
    }
    sheets.close()
    for (const file of picked) {
      const key = `draft-${(draftFileCount += 1)}`
      setDraftFiles((list) => [...list, { key, name: file.name, image: isImageType(file.mimeType) }])
      taskApi.uploadInline(client, workspaceId, id, file).then(
        (record) => setDraftFiles((list) => list.map((item) => (item.key === key ? { ...item, path: attachmentPath(record) } : item))),
        (error: unknown) => {
          setDraftFiles((list) => list.filter((item) => item.key !== key))
          Alert.alert(`Could not upload “${file.name}”`, errorMessage(error))
        },
      )
    }
  }

  const header = (
    <PaneHeader className="px-3">
      <IconButton icon="back" label="Back to tasks" color={alpha(colors.mutedForeground, 0.7)} onPress={close} />
      <View className="flex-1 flex-row items-center gap-1.5 min-w-0">
        {project ? (
          <Pressable accessibilityRole="link" onPress={() => router.navigate('/tasks')} className="flex-row items-center gap-1.5 shrink">
            <View className="size-1.5 rounded-full" style={{ backgroundColor: project.color }} />
            <Text numberOfLines={1} className="font-noto-medium text-[12px] leading-[16px] shrink">
              {project.name}
            </Text>
          </Pressable>
        ) : null}
        {project ? <Icon name="chevronRight" size={14} color={colors.mutedForeground} /> : null}
        {(current?.ancestors ?? []).slice(-2).map((ancestor) => (
          <Pressable key={ancestor.id} accessibilityRole="link" accessibilityLabel={`${refIdentifier(ancestor)} ${ancestor.title}`} onPress={() => router.push(`/tasks/${ancestor.id}`)} className="flex-row items-center gap-1.5">
            <Text className="text-[12px] leading-[16px] text-muted-foreground">{refIdentifier(ancestor)}</Text>
            <Icon name="chevronRight" size={14} color={colors.mutedForeground} />
          </Pressable>
        ))}
        <Text className="text-[12px] leading-[16px] text-muted-foreground">{current ? identifier(current, projects.data ?? []) : 'Task'}</Text>
      </View>
      {current ? (
        <Button
          label="Delete"
          variant="destructive"
          disabled={trash.isPending}
          onPress={() =>
            Alert.alert('Move to trash?', current.sub_issue_count > 0 ? 'Its sub-issues go to the trash with it. You can restore them from the trash.' : 'You can restore it from the trash.', [
              { text: 'Cancel', style: 'cancel' },
              { text: 'Delete', style: 'destructive', onPress: () => trash.mutate(current) },
            ])
          }
        />
      ) : null}
    </PaneHeader>
  )

  if (!current) {
    return (
      <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
        {header}
        {task.isError ? <ErrorView error={task.error} onRetry={() => void task.refetch()} /> : <LoadingView />}
      </SafeAreaView>
    )
  }

  const subject = identifier(current, projects.data ?? [])
  const status = statuses.data?.find((item) => item.id === current.status_id)
  const ownStatuses = statuses.data ? projectStatuses(statuses.data, current.project_id) : []
  const description = splitInlineFiles(current.description)
  const descriptionText = markdownToText(description.text)
  const taskFiles: ShownFile[] = [
    ...description.files.map((file) => ({ key: file.path, ...file })),
    ...(attachments.data ?? []).map((record) => ({ key: record.id, name: record.display_name, path: attachmentPath(record), image: isImageType(record.media_type), size: record.byte_size, deleted: record.deleted })),
  ]
  const taskLabels = (labels.data ?? []).filter((label) => current.label_ids.includes(label.id))
  const children = subIssues.data ?? []
  // a parent is never the task or a loaded sub-issue (deeper ones: the server's parent_cycle); an added sub-issue is
  // never the task, an ancestor or a sub-issue it already has
  const pickExcluded = (purpose: 'parent' | 'sub-issue' | RelationKind) => {
    if (purpose === 'parent') return [current.id, ...children.map((child) => child.id)]
    if (purpose === 'sub-issue') return [current.id, ...(current.ancestors ?? []).map((ref) => ref.id), ...children.map((child) => child.id)]
    if (purpose === 'duplicate') return [current.id]
    return [current.id, ...relatedTaskIds(relations.data ?? [])]
  }
  const choose = (purpose: 'parent' | 'sub-issue' | RelationKind, target: api.TaskRecord) => {
    sheets.close()
    if (purpose === 'parent') write.mutate({ parent_task_id: target.id })
    else if (purpose === 'sub-issue') adopt.mutate(target)
    else if (purpose === 'duplicate') write.mutate({ duplicate_of_id: target.id })
    else relationWrites.add.mutate({ type: purpose, task_id: target.id })
  }
  const filesOf = (comment: api.CommentRecord): ShownFile[] => [
    ...splitInlineFiles(comment.body).files.map((file) => ({ key: file.path, ...file })),
    ...(commentFiles.get(comment.id) ?? []).map((record) => ({ key: record.id, name: record.display_name, path: attachmentPath(record), image: isImageType(record.media_type), size: record.byte_size, deleted: record.deleted })),
  ]
  const uploading = draftFiles.some((file) => !file.path)
  const readyFiles = draftFiles.flatMap((file) => (file.path ? [{ name: file.name, path: file.path, image: file.image }] : []))
  const sheet = sheets.sheet

  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
      {header}
      <KeyboardAvoidingView className="flex-1" behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
        <ScrollView
          contentContainerClassName="px-3.5 pt-4 pb-7 gap-3.5"
          keyboardShouldPersistTaps="handled"
          refreshControl={
            <RefreshControl
              refreshing={task.isRefetching}
              onRefresh={() => void Promise.all([task.refetch(), comments.refetch(), activity.refetch(), relations.refetch(), subIssues.refetch(), attachments.refetch()])}
              tintColor={colors.mutedForeground}
            />
          }
        >
          <View>
            <DuplicateBanner task={current} status={status} pending={write.isPending} onUnmark={() => write.mutate({ duplicate_of_id: null })} />
            <Text selectable className="font-noto-semibold text-[20px] leading-[26px]">
              {current.title || 'Untitled'}
            </Text>
            {descriptionText ? (
              <Text selectable className="text-[16px] leading-[24px] mt-[19px]">
                {descriptionText}
              </Text>
            ) : null}
            <Attachments files={taskFiles} />
            <View className={`flex-row ${descriptionText || taskFiles.length > 0 ? 'mt-[43px]' : 'mt-3.5'}`}>
              <QuietAction icon="relation" label="Add relation" onPress={() => sheets.open({ kind: 'relation-menu' })} />
              {current.sub_issue_count === 0 && children.length === 0 && !composingSubIssue ? <QuietAction icon="subIssue" label="Add sub-issue" onPress={() => setComposingSubIssue(true)} /> : null}
            </View>
          </View>

          <SubIssuesSection
            tasks={children}
            statuses={statuses.data ?? []}
            projectId={current.project_id}
            composing={composingSubIssue}
            creating={createSubIssue.isPending}
            failed={createSubIssue.isError}
            onCompose={() => setComposingSubIssue(true)}
            onAddExisting={() => sheets.open({ kind: 'pick-task', purpose: 'sub-issue' })}
            onCloseComposer={() => setComposingSubIssue(false)}
            onCreate={(title) => {
              const body = subIssueBody(current, title, { statuses: statuses.data ?? [], userId: user.id, siblings: children })
              if (body) createSubIssue.mutate(body)
            }}
          />
          <RelationsSection
            relations={relations.data ?? []}
            statuses={statuses.data ?? []}
            removingId={relationWrites.remove.isPending ? relationWrites.remove.variables : undefined}
            onRemove={(relationId) => relationWrites.remove.mutate(relationId)}
          />

          <View className="border-t border-b border-border py-3.5 flex-row flex-wrap gap-x-3 gap-y-3.5">
            <PropertyGroup title="Properties" full>
              <View className="flex-row flex-wrap gap-1">
                <Chip pull label={`Status: ${status?.name ?? 'None'}`} onPress={ownStatuses.length > 0 ? () => sheets.open({ kind: 'status' }) : undefined}>
                  <StatusIcon status={status} size={16} />
                  <ChipText>{status?.name ?? 'No status'}</ChipText>
                </Chip>
                <Chip pull label={`Priority: ${PRIORITY_LABEL[priorityOf(current)]}`} onPress={() => sheets.open({ kind: 'priority' })}>
                  <PriorityIcon priority={current.priority} size={16} />
                  <ChipText>{priorityOf(current) === 'none' ? 'Set priority' : PRIORITY_LABEL[priorityOf(current)]}</ChipText>
                </Chip>
                <Chip pull label="Assignees" onPress={() => sheets.open({ kind: 'assignees' })}>
                  {current.assignee_ids.length === 0 ? (
                    <>
                      <Icon name="user" size={14} color={colors.mutedForeground} />
                      <ChipText muted>Assignee</ChipText>
                    </>
                  ) : (
                    <>
                      {current.assignee_ids.slice(0, 2).map((userId) => (
                        <Avatar key={userId} id={userId} name={nameOf(userId)} size={16} />
                      ))}
                      <ChipText>{current.assignee_ids.map(nameOf).join(', ')}</ChipText>
                    </>
                  )}
                </Chip>
              </View>
              <View className="flex-row mt-1 -ml-2">
                <Chip
                  label={current.parent ? `Parent: ${refIdentifier(current.parent)} ${current.parent.title}` : 'Set parent'}
                  onPress={() => sheets.open(current.parent ? { kind: 'parent-menu' } : { kind: 'pick-task', purpose: 'parent' })}
                >
                  <Icon name="subIssue" size={14} color={colors.mutedForeground} />
                  {current.parent ? (
                    <>
                      <ChipText muted>{refIdentifier(current.parent)}</ChipText>
                      <ChipText>{current.parent.title || 'Untitled'}</ChipText>
                    </>
                  ) : (
                    <ChipText muted>Set parent</ChipText>
                  )}
                </Chip>
              </View>
            </PropertyGroup>
            <PropertyGroup title="Labels">
              {taskLabels.length === 0 ? (
                <View className="-ml-2 flex-row">
                  <Chip label="Add label" onPress={() => sheets.open({ kind: 'labels' })}>
                    <Icon name="tag" size={14} color={colors.mutedForeground} />
                    <ChipText muted>Add label</ChipText>
                  </Chip>
                </View>
              ) : (
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel={`Labels: ${taskLabels.map((label) => label.name).join(', ')}`}
                  onPress={() => sheets.open({ kind: 'labels' })}
                  className="flex-row flex-wrap gap-1"
                >
                  {taskLabels.map((label) => (
                    <LabelPill key={label.id} label={label} />
                  ))}
                  <View className="size-5 items-center justify-center">
                    <Icon name="plus" size={12} color={colors.mutedForeground} />
                  </View>
                </Pressable>
              )}
            </PropertyGroup>
            <PropertyGroup title="Project">{project ? <Pill color={project.color} name={project.name} /> : <ChipText muted>—</ChipText>}</PropertyGroup>
            <PropertyGroup title="Source">
              {current.source_url ? (
                <Pressable accessibilityRole="link" accessibilityLabel={`Open source ${current.source_url}`} onPress={() => void openURL(current.source_url!)} className="max-w-full flex-row items-center gap-1 h-7">
                  <Icon name="arrowUpRightSquare" size={14} color={colors.mutedForeground} />
                  <ChipText>{sourceHost(current.source_url)}</ChipText>
                </Pressable>
              ) : null}
              <View className="-ml-2 flex-row">
                <Chip label={current.source_url ? 'Edit source' : 'Add source'} onPress={() => sheets.open({ kind: 'source' })}>
                  <ChipText muted>{current.source_url ? 'Edit source' : 'Add source'}</ChipText>
                </Chip>
              </View>
            </PropertyGroup>
            <PropertyGroup title="Due date">
              <View className="-ml-2 flex-row">
                <Chip label="Due date" onPress={() => sheets.open({ kind: 'due' })}>
                  <Icon name="calendar" size={14} color={colors.foreground} />
                  <ChipText>{dueDateLabel(current.due_at, current.due_start_at)}</ChipText>
                </Chip>
              </View>
            </PropertyGroup>
          </View>

          <View>
            <Text className="font-noto-semibold text-[14px] leading-[20px] mb-2">Activity</Text>
            {activity.isError || comments.isError ? (
              <Pressable onPress={() => void Promise.all([activity.refetch(), comments.refetch()])}>
                <Text className="text-[12px] text-destructive">Could not load the activity. Tap to retry.</Text>
              </Pressable>
            ) : (
              feed.map((entry) =>
                entry.kind === 'thread' ? (
                  <Thread key={entry.thread.root.id} thread={entry.thread} nameOf={nameOf} filesOf={filesOf} />
                ) : (
                  <Timeline
                    key={entry.items[0]!.id}
                    run={entry.items}
                    unfolded={unfolded.includes(entry.items[0]!.id)}
                    onToggle={() => {
                      const runId = entry.items[0]!.id
                      setUnfolded((list) => (list.includes(runId) ? list.filter((item) => item !== runId) : [...list, runId]))
                    }}
                    nameOf={nameOf}
                    statusOf={(statusId) => statuses.data?.find((item) => item.id === statusId)}
                  />
                ),
              )
            )}
            <CommentComposer
              value={draft}
              onChangeText={setDraft}
              files={draftFiles}
              onRemoveFile={(key) => setDraftFiles((list) => list.filter((file) => file.key !== key))}
              canSend={(draft.trim().length > 0 || readyFiles.length > 0) && !uploading && !sendComment.isPending}
              sending={sendComment.isPending}
              onSend={() => sendComment.mutate(bodyWithFiles(draft, readyFiles))}
              onAttach={() => sheets.open({ kind: 'attach' })}
            />
          </View>
        </ScrollView>
      </KeyboardAvoidingView>

      <Sheet visible={sheets.visible} title={sheetTitle(sheet, subject)} onClose={sheets.close}>
        {sheet?.kind === 'status' ? (
          <StatusChoices
            statuses={ownStatuses}
            value={current.status_id}
            onSelect={(statusId) => {
              sheets.close()
              if (statusId !== current.status_id) write.mutate({ status_id: statusId })
            }}
          />
        ) : null}
        {sheet?.kind === 'priority' ? (
          <PriorityChoices
            value={priorityOf(current)}
            onSelect={(priority) => {
              sheets.close()
              if (priority !== priorityOf(current)) write.mutate({ priority })
            }}
          />
        ) : null}
        {sheet?.kind === 'assignees' ? <PeopleChoices people={people} value={current.assignee_ids} onChange={(ids) => write.mutate({ assignee_ids: ids })} /> : null}
        {sheet?.kind === 'labels' ? <LabelChoices projectId={current.project_id} value={current.label_ids} onChange={(ids) => write.mutate({ label_ids: ids })} /> : null}
        {sheet?.kind === 'due' ? (
          <DueDateChoices
            dueAt={current.due_at}
            onChange={(due) => {
              sheets.close()
              write.mutate({ due_at: due?.end ?? null, due_start_at: due?.start ?? null })
            }}
          />
        ) : null}
        {sheet?.kind === 'source' ? (
          <SourceForm
            value={current.source_url}
            onSave={(url) => {
              sheets.close()
              if (url !== (current.source_url ?? null)) write.mutate({ source_url: url })
            }}
          />
        ) : null}
        {sheet?.kind === 'parent-menu' && current.parent ? (
          <MenuChoices
            items={[
              { key: 'open', label: 'Open parent', icon: 'arrowRight' },
              { key: 'change', label: 'Change parent…', icon: 'subIssue' },
              { key: 'remove', label: 'Remove parent', icon: 'close' },
            ]}
            onSelect={(key) => {
              if (key === 'change') return sheets.open({ kind: 'pick-task', purpose: 'parent' })
              sheets.close()
              if (key === 'open') router.push(`/tasks/${current.parent!.id}`)
              else write.mutate({ parent_task_id: null })
            }}
          />
        ) : null}
        {sheet?.kind === 'relation-menu' ? (
          <MenuChoices items={RELATION_OPTIONS.map((option) => ({ key: option.kind, label: option.label }))} onSelect={(kind) => sheets.open({ kind: 'pick-task', purpose: kind })} />
        ) : null}
        {sheet?.kind === 'pick-task' ? (
          <TaskChoices key={sheet.purpose} excludeIds={pickExcluded(sheet.purpose)} excludeDuplicates={sheet.purpose === 'duplicate'} onSelect={(target) => choose(sheet.purpose, target)} />
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
    </SafeAreaView>
  )
}

/** "Add relation" / "Add sub-issue": the web's quiet ghost buttons under the description. */
function QuietAction({ icon, label, onPress }: { icon: IconName; label: string; onPress: () => void }) {
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

function PropertyGroup({ title, full = false, children }: { title: string; full?: boolean; children: ReactNode }) {
  return (
    <View className={`items-start ${full ? 'w-full grow-0' : 'w-[47%] grow'}`}>
      <Text className={`font-noto-medium text-[12px] leading-[16px] text-muted-foreground/70 ${full ? 'mb-2.5' : 'mb-2'}`}>{title}</Text>
      {children}
    </View>
  )
}

/** The web `PropertyButton` at phone size: a 28px ghost button with 12px text. */
function Chip({ label, onPress, pull = false, children }: { label: string; onPress?: () => void; /** The web's `-ml-2`: each chip of a wrapping row starts 8px left. */ pull?: boolean; children: ReactNode }) {
  return (
    <Pressable
      accessibilityRole={onPress ? 'button' : undefined}
      accessibilityLabel={label}
      disabled={!onPress}
      onPress={onPress}
      className={`h-7 max-w-full flex-row items-center gap-1.5 px-1.5 rounded-lg active:bg-muted ${pull ? '-ml-2' : 'ml-0'}`}
    >
      {children}
    </Pressable>
  )
}

function ChipText({ children, muted = false }: { children: ReactNode; muted?: boolean }) {
  return (
    <Text numberOfLines={1} className={`font-noto-medium text-[12px] leading-[16px] shrink ${muted ? 'text-muted-foreground' : 'text-foreground'}`}>
      {children}
    </Text>
  )
}

/** A project or label: a round outline pill with its color dot. */
function Pill({ color, name }: { color: string; name: string }) {
  return (
    <View className="h-5 flex-row items-center gap-1 px-2 rounded-full border border-input bg-input/30">
      <View className="size-1.5 rounded-full" style={{ backgroundColor: color }} />
      <Text numberOfLines={1} className="font-noto-medium text-[12px] leading-[16px]">
        {name}
      </Text>
    </View>
  )
}

/** A run of activity rows on a connector line, folded to its first and latest rows like the web. */
function Timeline({
  run,
  unfolded,
  onToggle,
  nameOf,
  statusOf,
}: {
  run: ActivityRow[]
  unfolded: boolean
  onToggle: () => void
  nameOf: (userId: string) => string
  statusOf: (statusId: string) => api.StatusRecord | undefined
}) {
  const { colors } = useTheme()
  const folded = foldRun(run)
  const items = unfolded ? run : folded.items
  const rows: ReactNode[] = []
  items.forEach((item, index) => {
    const status = item.statusId ? statusOf(item.statusId) : undefined
    const actor = item.actorName ?? (item.actorId ? nameOf(item.actorId) : 'Someone')
    rows.push(
      <TimelineRow
        key={item.id}
        first={index === 0}
        last={index === items.length - 1 && folded.hidden === 0}
        glyph={status ? <StatusIcon status={status} size={14} /> : <Avatar id={item.actorId} name={actor} size={14} />}
      >
        <Text numberOfLines={1} className="text-[12px] leading-[16px] flex-1 text-muted-foreground">
          <Text className="font-noto-medium text-[12px] leading-[16px] text-foreground">{actor}</Text> {item.text} · {relativeAgo(item.createdAt)}
        </Text>
      </TimelineRow>,
    )
    if (folded.hidden > 0 && index === 0) {
      rows.push(
        <TimelineRow key={`${item.id}-fold`} first={false} last={items.length === 1} glyph={null}>
          <Pressable accessibilityRole="button" accessibilityState={{ expanded: unfolded }} onPress={onToggle} className="flex-row items-center gap-2">
            <View className={unfolded ? 'rotate-180' : undefined}>
              <Icon name="chevronDown" size={14} color={colors.mutedForeground} />
            </View>
            <Text className="font-noto-medium text-[12px] leading-[16px] text-muted-foreground">{unfolded ? 'Show less' : `Show ${folded.hidden} more`}</Text>
          </Pressable>
        </TimelineRow>,
      )
    }
  })
  return <View>{rows}</View>
}

/** One timeline row: the 1px connector runs above and below the 14px glyph, the first and last drop their outer half. */
function TimelineRow({ first, last, glyph, children }: { first: boolean; last: boolean; glyph: ReactNode; children: ReactNode }) {
  return (
    <View className="min-h-7.5 flex-row items-center gap-2.5 py-[7px] pl-1.5">
      {first ? null : <View className="absolute top-0 left-[12.5px] h-[7px] w-px bg-border" />}
      {last ? null : <View className="absolute bottom-0 left-[12.5px] h-[7px] w-px bg-border" />}
      <View className="size-3.5 items-center justify-center">{glyph}</View>
      {children}
    </View>
  )
}

/** A comment and its replies in one card. */
function Thread({ thread, nameOf, filesOf }: { thread: CommentThread; nameOf: (userId: string) => string; filesOf: (comment: api.CommentRecord) => ShownFile[] }) {
  return (
    <View className="my-3 rounded-lg border border-border bg-card/50 overflow-hidden">
      {[thread.root, ...thread.replies].map((comment, index) => (
        <View key={comment.id} className={`flex-row gap-2 px-3.5 py-2.5 border-border ${index === 0 ? 'border-t-0' : 'border-t'}`}>
          <Avatar id={comment.author_id} name={nameOf(comment.author_id)} size={20} />
          <View className="flex-1 min-w-0">
            <View className="min-h-5 flex-row items-center gap-1.5">
              <Text numberOfLines={1} className="font-noto-semibold text-[13px] shrink">
                {nameOf(comment.author_id)}
              </Text>
              <Text className="text-[11px] text-muted-foreground/70 leading-[21px]">·</Text>
              <Text className="font-noto-medium text-[11px] text-muted-foreground">
                {relativeAgo(comment.created_at)}
                {comment.updated_at !== comment.created_at ? ' (edited)' : ''}
              </Text>
            </View>
            <CommentBody comment={comment} files={filesOf(comment)} />
          </View>
        </View>
      ))}
    </View>
  )
}

/** A comment's text, then its files (inline ones linked from the text, then attached ones). */
function CommentBody({ comment, files }: { comment: api.CommentRecord; files: ShownFile[] }) {
  const text = markdownToText(splitInlineFiles(comment.body).text)
  return (
    <>
      {text ? (
        <Text selectable className="text-[13px] leading-[19px] mt-0.5">
          {text}
        </Text>
      ) : null}
      <Attachments files={files} />
    </>
  )
}

/** The web comment box: a card with the field on top, the files to send, and attach + a round send button below. */
function CommentComposer({
  value,
  onChangeText,
  files,
  onRemoveFile,
  canSend,
  sending,
  onSend,
  onAttach,
}: {
  value: string
  onChangeText: (text: string) => void
  files: readonly ShownFile[]
  onRemoveFile: (key: string) => void
  canSend: boolean
  sending: boolean
  onSend: () => void
  onAttach: () => void
}) {
  const { colors, scheme } = useTheme()
  const [focused, setFocused] = useState(false)
  return (
    <View className={`mt-3 rounded-lg border bg-card/50 ${focused ? 'border-ring/50' : 'border-border'}`}>
      <TextInput
        value={value}
        onChangeText={onChangeText}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        placeholder="Leave a comment… Use @ to mention"
        placeholderTextColor={colors.mutedForeground}
        selectionColor={colors.primary}
        keyboardAppearance={scheme}
        multiline
        numberOfLines={1}
        accessibilityLabel="Comment"
        className="min-h-10 max-h-40 px-3 pt-2.5 pb-0 font-noto text-[16px] leading-[24px] text-foreground"
      />
      {files.length > 0 ? (
        <View className="px-3">
          <Attachments files={files} onRemove={onRemoveFile} />
        </View>
      ) : null}
      <View className="flex-row justify-end items-center gap-1 p-1.5">
        <IconButton icon="attach" label="Attach" color={colors.mutedForeground} onPress={onAttach} />
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={sending ? 'Sending…' : 'Send'}
          accessibilityState={{ disabled: !canSend }}
          disabled={!canSend}
          onPress={onSend}
          className={`size-7 rounded-full items-center justify-center bg-primary ${canSend ? 'opacity-100' : 'opacity-50'}`}
        >
          <Icon name="arrowUp" size={16} color={colors.primaryForeground} />
        </Pressable>
      </View>
    </View>
  )
}
