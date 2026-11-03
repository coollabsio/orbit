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
  return (
    <View className="flex-1 items-center justify-center gap-2 p-6">
      {icon ? (
        <View className="mb-2 size-12 items-center justify-center rounded-xl bg-muted">
          <Icon name={icon} size={24} color={colors.mutedForeground} />
        </View>
      ) : null}
      <Text className="text-center font-noto-semibold text-[17px] leading-[22px]">{title}</Text>
      {description ? <Text className="max-w-[300px] text-center leading-[21px] text-muted-foreground">{description}</Text> : null}
      {action ? <View className="mt-2">{action}</View> : null}
    </View>
  )
}
