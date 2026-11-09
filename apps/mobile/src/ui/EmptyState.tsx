import type { ReactNode } from 'react'
import { View } from 'react-native'

import { useTheme } from '../theme'
import { Icon, type IconName } from './Icon'
import { Text } from './Text'

export interface EmptyStateProps {
  title: string
  description?: string
  icon?: IconName
  /** A button or link below the text. */
  action?: ReactNode
}

export function EmptyState({ title, description, icon, action }: EmptyStateProps) {
  const { colors } = useTheme()
  // the web's `EmptyState` (shadcn `Empty`): a 32px icon tile, 14px medium title, 14px relaxed description
  return (
    <View className="flex-1 items-center justify-center gap-4 p-6">
      <View className="max-w-[384px] items-center gap-2">
        {icon ? (
          <View className="mb-2 size-8 items-center justify-center rounded-lg bg-muted">
            <Icon name={icon} size={20} color={colors.foreground} />
          </View>
        ) : null}
        <Text className="text-center font-noto-medium text-[14px] tracking-tight">{title}</Text>
        {description ? <Text className="text-center text-[14px] leading-[22.75px] text-muted-foreground">{description}</Text> : null}
      </View>
      {action ? <View className="items-center">{action}</View> : null}
    </View>
  )
}
