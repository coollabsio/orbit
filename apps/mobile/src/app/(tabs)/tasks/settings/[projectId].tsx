import { router, useLocalSearchParams } from 'expo-router'
import { useEffect, useState } from 'react'
import { Alert, Pressable, ScrollView, View } from 'react-native'

import { ActionSheet, ConfirmDialog, type ConfirmRequest, Dialog, type SheetAction } from '../../../../components/settings/Dialog'
import { Badge, Note, RowText, SettingsCard, SettingsRow, SmallOutlineButton } from '../../../../components/settings/Shell'
import { DayPicker } from '../../../../components/tasks/DayPicker'
import { ProjectDocsCard } from '../../../../components/tasks/ProjectDocsCard'
import type { TaskCycle, TaskMilestone } from '../../../../data/task-api'
import {
  cycleDates,
  cycleErrorMessage,
  cycleLabel,
  cycleLastDay,
  changedCycleSpan,
  cycleTiming,
  isMilestoneLate,
  isNoNextCycle,
  milestoneErrorMessage,
  nextCycleSpan,
  progressLabel,
  sortCycles,
  sortMilestones,
  targetDay,
  targetValue,
} from '../../../../data/tasks'
import { useCycleWrites, useMilestoneWrites } from '../../../../hooks/tasks'
import { useCycles, useMilestones, useProjects } from '../../../../queries'
import { alpha, useTheme } from '../../../../theme'
import { Button, Icon, IconButton, Input, KeyboardSpace, PaneHeader, PullRefresh, SafeAreaView, Text } from '../../../../ui'

const plural = (count: number, noun: string) => `${count} ${noun}${count === 1 ? '' : 's'}`
const longDate = (date: Date) => date.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })

type Menu = { kind: 'milestone'; milestone: TaskMilestone } | { kind: 'cycle'; cycle: TaskCycle }
/** The open form: `item` null for a new one. */
type Form = { kind: 'milestone'; milestone: TaskMilestone | null } | { kind: 'cycle'; cycle: TaskCycle | null }

/**
 * A project's settings on the phone: its milestones (progress, target date, order) and its cycles (dates, the current
 * one, moving unfinished tasks on) and its docs teamspace. Opened from the project's task list. Statuses and labels stay
 * on the web.
 */
export default function ProjectSettingsScreen() {
  const { projectId } = useLocalSearchParams<{ projectId: string }>()
  const { colors } = useTheme()
  const projects = useProjects()
  const milestones = useMilestones(projectId)
  const cycles = useCycles(projectId)
  const milestoneWrites = useMilestoneWrites(projectId)
  const cycleWrites = useCycleWrites(projectId)
  const [menu, setMenu] = useState<Menu | null>(null)
  const [form, setForm] = useState<Form | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const project = projects.data?.find((item) => item.id === projectId)
  const orderedMilestones = sortMilestones(milestones.data ?? [])
  const orderedCycles = sortCycles(cycles.data ?? [])
  const now = new Date()

  const back = () => (router.canGoBack() ? router.back() : router.navigate({ pathname: '/tasks', params: { project: projectId } }))

  const move = (milestone: TaskMilestone, step: -1 | 1) => {
    const list = [...orderedMilestones]
    const from = list.findIndex((item) => item.id === milestone.id)
    const to = from + step
    if (from < 0 || to < 0 || to >= list.length) return
    ;[list[from], list[to]] = [list[to]!, list[from]!]
    milestoneWrites.reorder.mutate(list, { onError: (error) => Alert.alert('Could not reorder the milestones', milestoneErrorMessage(error, 'Try again.')) })
  }

  const moveUnfinished = (cycle: TaskCycle) =>
    cycleWrites.moveUnfinished.mutate(cycle, {
      onSuccess: ({ moved, to_cycle }) =>
        Alert.alert('Tasks moved', moved === 0 ? `${cycleLabel(cycle)} has no unfinished tasks.` : `Moved ${plural(moved, 'unfinished task')} to ${cycleLabel(to_cycle)}.`),
      onError: (error) =>
        isNoNextCycle(error)
          ? Alert.alert('No next cycle', `${cycleLabel(cycle)} is the last cycle of this project. Create the next cycle first, then move the unfinished tasks.`, [
              { text: 'Cancel', style: 'cancel' },
              { text: 'Create cycle', onPress: () => setForm({ kind: 'cycle', cycle: null }) },
            ])
          : Alert.alert('Could not move the tasks', cycleErrorMessage(error, 'Try again.')),
    })

  const menuActions = (): SheetAction[] => {
    if (!menu) return []
    if (menu.kind === 'milestone') {
      const { milestone } = menu
      const index = orderedMilestones.findIndex((item) => item.id === milestone.id)
      return [
        { label: 'Edit', icon: 'edit', onPress: () => setForm({ kind: 'milestone', milestone }) },
        { label: 'Move up', icon: 'arrowUp', disabled: index <= 0, onPress: () => move(milestone, -1) },
        { label: 'Move down', icon: 'arrowDown', disabled: index < 0 || index >= orderedMilestones.length - 1, onPress: () => move(milestone, 1) },
        {
          label: 'Delete',
          icon: 'trash',
          destructive: true,
          onPress: () =>
            setConfirm({
              title: `Delete “${milestone.name}”?`,
              description: milestone.task_count > 0 ? `Its ${plural(milestone.task_count, 'task')} stay and lose the milestone.` : 'No task is in this milestone.',
              confirmLabel: 'Delete',
              danger: true,
              onConfirm: () => milestoneWrites.remove.mutate(milestone, { onError: (error) => Alert.alert('Could not delete the milestone', milestoneErrorMessage(error, 'Try again.')) }),
            }),
        },
      ]
    }
    const { cycle } = menu
    const unfinished = cycle.task_count - cycle.completed_count
    return [
      { label: 'Edit', icon: 'edit', onPress: () => setForm({ kind: 'cycle', cycle }) },
      {
        label: 'Move unfinished tasks to next cycle',
        icon: 'forward',
        disabled: unfinished === 0 || cycleWrites.moveUnfinished.isPending,
        detail: unfinished === 0 ? 'Every task of this cycle is done.' : `${plural(unfinished, 'task')} not done yet`,
        onPress: () => moveUnfinished(cycle),
      },
      {
        label: 'Delete',
        icon: 'trash',
        destructive: true,
        onPress: () =>
          setConfirm({
            title: `Delete ${cycleLabel(cycle)}?`,
            description: cycle.task_count > 0 ? `Its ${plural(cycle.task_count, 'task')} stay and leave the cycle.` : 'No task is in this cycle.',
            confirmLabel: 'Delete',
            danger: true,
            onConfirm: () => cycleWrites.remove.mutate(cycle, { onError: (error) => Alert.alert('Could not delete the cycle', cycleErrorMessage(error, 'Try again.')) }),
          }),
      },
    ]
  }

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <IconButton icon="back" label="Back to tasks" color={alpha(colors.mutedForeground, 0.7)} onPress={back} />
        <View className="flex-1 flex-row items-center gap-1.5 min-w-0">
          {project ? <View className="size-1.5 rounded-full" style={{ backgroundColor: project.color }} /> : null}
          <Text numberOfLines={1} className="font-noto-medium text-[13px] shrink text-muted-foreground">
            {project?.name ?? 'Project'}
          </Text>
          <Text className="text-[13px] text-muted-foreground">/</Text>
          <Text accessibilityRole="header" className="font-noto-semibold text-[13px]">
            Settings
          </Text>
        </View>
      </PaneHeader>
      <KeyboardSpace className="flex-1">
        <ScrollView
          contentContainerClassName="px-5 pt-5 pb-8 gap-6"
          keyboardShouldPersistTaps="handled"
          refreshControl={<PullRefresh onRefresh={() => Promise.all([milestones.refetch(), cycles.refetch()])} />}
        >
          <SettingsCard
            title="Milestones"
            description="Steps toward the project's goal, in order. Progress counts the done tasks of each."
            flush={orderedMilestones.length > 0}
            actions={<SmallOutlineButton label="New milestone" icon="plus" onPress={() => setForm({ kind: 'milestone', milestone: null })} />}
          >
            {milestones.isPending ? <Note loading>Loading milestones…</Note> : null}
            {milestones.isError ? <Note error>Could not load the milestones. Pull to retry.</Note> : null}
            {milestones.isSuccess && orderedMilestones.length === 0 ? <Note>No milestones yet.</Note> : null}
            {orderedMilestones.map((milestone, index) => (
              <SettingsRow key={milestone.id} first={index === 0} label={`${milestone.name}, ${progressLabel(milestone)} done${isMilestoneLate(milestone, now) ? ', late' : ''}`} onPress={() => setMenu({ kind: 'milestone', milestone })}>
                <Icon name="target" size={16} color={colors.mutedForeground} />
                <RowText
                  title={milestone.name}
                  badges={isMilestoneLate(milestone, now) ? <Text className="text-[12px] text-destructive">Late</Text> : null}
                  subtitle={[milestone.target_at ? `Target ${longDate(targetDay(milestone.target_at))}` : null, milestone.description].filter(Boolean).join(' · ') || undefined}
                />
                <Text className="font-noto-medium text-[12px] text-muted-foreground">{progressLabel(milestone)}</Text>
              </SettingsRow>
            ))}
          </SettingsCard>

          <SettingsCard
            title="Cycles"
            description="Time boxes for the project's work. Tasks that are not done can move on to the next cycle."
            flush={orderedCycles.length > 0}
            actions={<SmallOutlineButton label="New cycle" icon="plus" onPress={() => setForm({ kind: 'cycle', cycle: null })} />}
          >
            {cycles.isPending ? <Note loading>Loading cycles…</Note> : null}
            {cycles.isError ? <Note error>Could not load the cycles. Pull to retry.</Note> : null}
            {cycles.isSuccess && orderedCycles.length === 0 ? <Note>No cycles yet.</Note> : null}
            {orderedCycles.map((cycle, index) => {
              const timing = cycleTiming(cycle, now)
              return (
                <SettingsRow key={cycle.id} first={index === 0} label={`${cycleLabel(cycle)}, ${cycleDates(cycle)}${timing === 'current' ? ', current' : ''}`} onPress={() => setMenu({ kind: 'cycle', cycle })}>
                  <Icon name="refresh" size={16} color={timing === 'current' ? colors.primary : colors.mutedForeground} />
                  <RowText
                    title={cycleLabel(cycle)}
                    subtitle={cycleDates(cycle)}
                    badges={timing === 'current' ? <Badge label="Current" /> : timing === 'past' ? <Badge label="Ended" tone="outline" /> : null}
                  />
                  <Text className="font-noto-medium text-[12px] text-muted-foreground">{progressLabel(cycle)}</Text>
                </SettingsRow>
              )
            })}
          </SettingsCard>

          {project ? <ProjectDocsCard project={project} /> : null}
        </ScrollView>
      </KeyboardSpace>

      <ActionSheet
        visible={menu !== null}
        title={menu ? (menu.kind === 'milestone' ? menu.milestone.name : cycleLabel(menu.cycle)) : undefined}
        actions={menuActions()}
        onClose={() => setMenu(null)}
      />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
      {form?.kind === 'milestone' ? (
        <MilestoneForm
          milestone={form.milestone}
          onClose={() => setForm(null)}
          onSave={(values) =>
            form.milestone
              ? milestoneWrites.update.mutateAsync({ milestone: form.milestone, changes: values })
              : milestoneWrites.create.mutateAsync({ name: values.name, description: values.description, target_at: values.target_at })
          }
        />
      ) : null}
      {form?.kind === 'cycle' ? (
        <CycleForm
          cycle={form.cycle}
          others={(cycles.data ?? []).filter((item) => item.id !== form.cycle?.id)}
          nextNumber={Math.max(0, ...(cycles.data ?? []).map((item) => item.number)) + 1}
          onClose={() => setForm(null)}
          onSave={(values, span) => (form.cycle ? cycleWrites.update.mutateAsync({ cycle: form.cycle, changes: values }) : cycleWrites.create.mutateAsync({ ...span, ...values }))}
        />
      ) : null}
    </SafeAreaView>
  )
}

/** Shows a write's error under the form and keeps it open; closes it once the write is done. */
function useSubmit(save: () => Promise<unknown>, onClose: () => void, message: (error: unknown) => string) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const submit = () => {
    if (busy) return
    setBusy(true)
    setError(null)
    save().then(onClose, (reason: unknown) => {
      setBusy(false)
      setError(message(reason))
    })
  }
  return { busy, error, submit }
}

/** New milestone or Edit milestone: name, description and an optional target date. */
function MilestoneForm({
  milestone,
  onSave,
  onClose,
}: {
  milestone: TaskMilestone | null
  onSave: (values: { name: string; description: string | null; target_at: string | null }) => Promise<unknown>
  onClose: () => void
}) {
  const [name, setName] = useState(milestone?.name ?? '')
  const [description, setDescription] = useState(milestone?.description ?? '')
  const [target, setTarget] = useState<Date | null>(milestone?.target_at ? targetDay(milestone.target_at) : null)
  const { busy, error, submit } = useSubmit(
    () => onSave({ name: name.trim(), description: description.trim() || null, target_at: targetValue(target) }),
    onClose,
    (reason) => milestoneErrorMessage(reason, 'The milestone could not be saved. Try again.'),
  )
  return (
    <Dialog
      visible
      title={milestone ? 'Edit milestone' : 'New milestone'}
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={milestone ? 'Save' : 'Create milestone'} loading={busy} disabled={!name.trim()} onPress={submit} />
        </>
      }
    >
      <Input label="Name" value={name} onChangeText={setName} placeholder="Beta release" autoFocus={!milestone} returnKeyType="next" />
      <Input label="Description" value={description} onChangeText={setDescription} placeholder="What this milestone delivers" multiline />
      <DateField label="Target date" value={target} onChange={setTarget} clearLabel="No target date" />
      {error ? <Text className="text-[13px] leading-[19px] text-destructive">{error}</Text> : null}
    </Dialog>
  )
}

/**
 * New cycle or Edit cycle: an optional name and its first and last day. A new cycle starts where the latest one ends
 * and lasts as long.
 */
function CycleForm({
  cycle,
  others,
  nextNumber,
  onSave,
  onClose,
}: {
  cycle: TaskCycle | null
  others: readonly TaskCycle[]
  nextNumber: number
  /** `values` has only the dates picked anew (a day left as shown keeps its stored instant); `span` is the one the form started from. */
  onSave: (values: { name: string | null; starts_at?: string; ends_at?: string }, span: Pick<TaskCycle, 'starts_at' | 'ends_at'>) => Promise<unknown>
  onClose: () => void
}) {
  const [span] = useState(() => cycle ?? nextCycleSpan(others, new Date()))
  const [name, setName] = useState(cycle?.name ?? '')
  const [first, setFirst] = useState<Date>(() => new Date(span.starts_at))
  const [last, setLast] = useState<Date>(() => cycleLastDay(span))
  const backwards = last.getTime() < first.getTime()
  const { busy, error, submit } = useSubmit(
    () => onSave({ name: name.trim() || null, ...changedCycleSpan(span, first, last) }, span),
    onClose,
    (reason) => cycleErrorMessage(reason, 'The cycle could not be saved. Try again.'),
  )
  return (
    <Dialog
      visible
      title={cycle ? `Edit ${cycleLabel(cycle)}` : 'New cycle'}
      onClose={onClose}
      footer={
        <>
          <Button label="Cancel" variant="outline" onPress={onClose} />
          <Button label={cycle ? 'Save' : 'Create cycle'} loading={busy} disabled={backwards} onPress={submit} />
        </>
      }
    >
      <Input label="Name" value={name} onChangeText={setName} placeholder={`Cycle ${cycle?.number ?? nextNumber}`} description="Optional. Without a name, the cycle shows its number." />
      <DateField label="First day" value={first} onChange={(day) => day && setFirst(day)} />
      <DateField label="Last day" value={last} onChange={(day) => day && setLast(day)} />
      {backwards ? <Text className="text-[13px] leading-[19px] text-destructive">The last day can’t be before the first day.</Text> : null}
      {error ? <Text className="text-[13px] leading-[19px] text-destructive">{error}</Text> : null}
    </Dialog>
  )
}

/** A day under a label: tap for the system calendar; `clearLabel` adds a way back to no date. */
function DateField({ label, value, onChange, clearLabel }: { label: string; value: Date | null; onChange: (day: Date | null) => void; clearLabel?: string }) {
  const { colors } = useTheme()
  const [picking, setPicking] = useState(false)
  // a picked day closes the calendar
  useEffect(() => setPicking(false), [value])
  return (
    <View className="gap-2">
      <Text className="font-noto-medium text-[14px] leading-[19.25px]">{label}</Text>
      <View className="flex-row items-center gap-2">
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`${label}: ${value ? longDate(value) : clearLabel ?? 'none'}`}
          onPress={() => setPicking((open) => !open)}
          className="h-8 flex-1 flex-row items-center gap-2 rounded-lg border border-input px-2.5 dark:bg-input/30 active:bg-input/50"
        >
          <Icon name="calendar" size={14} color={colors.mutedForeground} />
          <Text className={`text-[14px] ${value ? 'text-foreground' : 'text-muted-foreground'}`}>{value ? longDate(value) : clearLabel}</Text>
        </Pressable>
        {clearLabel && value ? <IconButton icon="close" label={`Clear ${label.toLowerCase()}`} color={colors.mutedForeground} onPress={() => onChange(null)} /> : null}
      </View>
      {picking ? <DayPicker value={value ?? new Date()} onPick={onChange} onCancel={() => setPicking(false)} /> : null}
    </View>
  )
}
