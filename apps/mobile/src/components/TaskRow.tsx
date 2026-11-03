import { Pressable, View } from 'react-native'

import type { Person } from '../data/chat'
import type { StatusRecord, TaskRecord } from '../data/orbit'
import { Avatar, PriorityIcon, StatusIcon, Text } from '../ui'

export interface TaskRowProps {
  task: TaskRecord
  status: StatusRecord | undefined
  assignee: Person | undefined
  onPress: () => void
}

/**
 * The web list row at phone width: the 28px select strip, priority, the tree gutter, status, title, assignee. The
 * identifier column is hidden below 480px on the web too.
 */
export function TaskRow({ task, status, assignee, onPress }: TaskRowProps) {
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
      <Text numberOfLines={1} className="font-noto-medium text-[13px] flex-1">
        {task.title || 'Untitled'}
      </Text>
      <View className="w-7 items-end">{assignee ? <Avatar id={assignee.id} name={assignee.name} size={18} /> : null}</View>
    </Pressable>
  )
}

/** The 22px ghost button the web wraps each row picker in. */
function Glyph({ children }: { children: React.ReactNode }) {
  return <View className="size-5.5 items-center justify-center">{children}</View>
}
