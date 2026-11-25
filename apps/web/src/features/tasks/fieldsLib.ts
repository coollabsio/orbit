import { AngleDownCircle, Calendar, CheckSquare, Hashtag, Link, ListCheck, TextalignLeft, type IconComponent } from 'reicon-react'
import type { TaskFieldKind, TaskFieldOption } from '@orbit/api/generated/types.gen'
import type { Task } from '@/features/tasks/api/models'
import { LABEL_COLORS } from '@/features/tasks/components/TaskLabels'

export { FIELD_KINDS, fieldKindLabel, fieldValueText, hasOptions, parseFieldInput, selectedOptions } from '@orbit/api/taskFields'

/** The next option's colour: the palette in turn. */
export function nextOptionColor(options: readonly Pick<TaskFieldOption, 'color'>[]): string {
  return LABEL_COLORS[options.length % LABEL_COLORS.length]!
}

/** The mark of a field kind, in pickers that list fields. */
export const FIELD_KIND_ICON: Record<TaskFieldKind, IconComponent> = {
  text: TextalignLeft,
  number: Hashtag,
  date: Calendar,
  select: AngleDownCircle,
  multi_select: ListCheck,
  checkbox: CheckSquare,
  url: Link,
}

/** The stored value of one custom field on a task; undefined when it has none. */
export function taskFieldValue(task: Pick<Task, 'fieldValues'>, fieldId: string): unknown {
  return task.fieldValues?.find((item) => item.field_id === fieldId)?.value
}
