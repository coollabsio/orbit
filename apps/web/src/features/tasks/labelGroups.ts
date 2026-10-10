import type { LabelRecord } from '@/api/generated/types.gen'
import type { FilterGroup } from '@/features/views/viewState'

type GroupedLabel = Pick<LabelRecord, 'id' | 'name' | 'group_id' | 'group_name'>

/** `Group / Label` for a label of a group, else the label's name. */
export function labelDisplayName(label: { name: string; group_name?: string | null }): string {
  return label.group_name ? `${label.group_name} / ${label.name}` : label.name
}

/** `labelIds` with `labelId` added. A task has one label of a group: the other label of the same group goes (the server does the same). */
export function addLabel(labelIds: string[], labelId: string, labels: GroupedLabel[]): string[] {
  const groupId = labels.find((label) => label.id === labelId)?.group_id
  const siblings = groupId ? labels.filter((label) => label.group_id === groupId && label.id !== labelId).map((label) => label.id) : []
  return [...labelIds.filter((id) => id !== labelId && !siblings.includes(id)), labelId]
}

export function toggleLabel(labelIds: string[], labelId: string, labels: GroupedLabel[]): string[] {
  return labelIds.includes(labelId) ? labelIds.filter((id) => id !== labelId) : addLabel(labelIds, labelId, labels)
}

export interface LabelSection<T> {
  groupId: string | null
  /** The group's name; null for the labels with no group. */
  name: string | null
  labels: T[]
}

/** Labels by group for a picker: the groups by name, then the labels with no group. Labels sort by name. */
export function labelSections<T extends GroupedLabel>(labels: T[]): LabelSection<T>[] {
  const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name)
  const groups = new Map<string, LabelSection<T>>()
  const loose: T[] = []
  for (const label of [...labels].sort(byName)) {
    if (!label.group_id) loose.push(label)
    else if (groups.has(label.group_id)) groups.get(label.group_id)!.labels.push(label)
    else groups.set(label.group_id, { groupId: label.group_id, name: label.group_name ?? 'Group', labels: [label] })
  }
  const sections = [...groups.values()].sort((a, b) => (a.name ?? '').localeCompare(b.name ?? ''))
  return loose.length > 0 ? [...sections, { groupId: null, name: null, labels: loose }] : sections
}

/** The tasks that stop `labelId` from joining a group: they have it and one of the group's labels. */
export function joinConflictFilter(labelId: string, groupLabelIds: string[]): FilterGroup {
  return {
    op: 'and',
    children: [
      { field: 'label', operator: 'includes_any', value: [labelId] },
      { field: 'label', operator: 'includes_any', value: groupLabelIds },
    ],
  }
}
