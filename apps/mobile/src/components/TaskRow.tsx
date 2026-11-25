import { Pressable, View } from 'react-native'

import type { Person } from '../data/chat'
import type { StatusRecord, TaskRecord } from '../data/orbit'
import type { FieldChip } from '../data/task-display'
import { useTheme } from '../theme'
import { Avatar, Icon, PriorityIcon, StatusIcon, Text } from '../ui'
import { LabelPill } from './tasks/pickers'

export interface TaskRowProps {
  task: TaskRecord
  status: StatusRecord | undefined
  assignee: Person | undefined
  /** The view's custom field values (`taskFieldChips`), on a line under the title; none adds no line. */
  fieldChips?: readonly FieldChip[]
  onPress: () => void
}

/**
 * The web list row at phone width: the 28px select strip, priority, the tree gutter, status, title, assignee. The
 * identifier column is hidden below 480px on the web too. Custom field values wrap under the title, as the web's
 * chips would not fit beside it at phone width.
 */
export function TaskRow({ task, status, assignee, fieldChips = [], onPress }: TaskRowProps) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={task.title}
      onPress={onPress}
      className="min-h-10 flex-row items-center gap-1.5 pr-3 py-1.5 border-b border-border active:bg-foreground/2"
    >
      <View className="w-7" />
      <Glyph>
        <PriorityIcon priority={task.priority} size={16} />
      </Glyph>
      <View className="w-4" />
      <Glyph>
        <StatusIcon status={status} size={16} />
      </Glyph>
      <View className="flex-1 min-w-0 gap-1">
        <Text numberOfLines={1} className="font-noto-medium text-[13px]">
          {task.title || 'Untitled'}
        </Text>
        {fieldChips.length > 0 ? (
          <View className="flex-row flex-wrap items-center gap-x-2 gap-y-1">
            {fieldChips.map((chip) => (
              <FieldValueChip key={chip.fieldId} chip={chip} />
            ))}
          </View>
        ) : null}
      </View>
      <View className="w-7 items-end">{assignee ? <Avatar id={assignee.id} name={assignee.name} size={18} /> : null}</View>
    </Pressable>
  )
}

/** The 22px ghost button the web wraps each row picker in. */
function Glyph({ children }: { children: React.ReactNode }) {
  return <View className="size-5.5 items-center justify-center">{children}</View>
}

/** One custom field value, as the web's `CustomFieldChip`: option pills (then "+n"), or the kind's icon and a short text. */
function FieldValueChip({ chip }: { chip: FieldChip }) {
  const { colors } = useTheme()
  if (chip.kind === 'options') {
    return (
      <View accessible accessibilityLabel={chip.label} className="flex-row items-center gap-1 max-w-[220px]">
        {chip.options.map((option) => (
          <View key={option.id} className="shrink">
            <LabelPill label={option} />
          </View>
        ))}
        {chip.more > 0 ? <Text className="text-[12px] leading-[16px] text-muted-foreground">+{chip.more}</Text> : null}
      </View>
    )
  }
  return (
    <View accessible accessibilityLabel={chip.label} className="flex-row items-center gap-1 max-w-[140px]">
      <Icon name={chip.icon} size={13} color={colors.mutedForeground} />
      <Text numberOfLines={1} className="shrink text-[12px] leading-[16px] text-muted-foreground">
        {chip.text}
      </Text>
    </View>
  )
}
