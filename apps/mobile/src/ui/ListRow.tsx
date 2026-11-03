import type { ReactNode } from 'react'
import { Pressable, View } from 'react-native'

import { Text } from './Text'

export interface ListRowProps {
  title: string
  subtitle?: string
  leading?: ReactNode
  trailing?: ReactNode
  onPress?: () => void
  /** A 1px line under the row, inset to the text. */
  separator?: boolean
}

export function ListRow({ title, subtitle, leading, trailing, onPress, separator = false }: ListRowProps) {
  return (
    <Pressable accessibilityRole={onPress ? 'button' : undefined} disabled={!onPress} onPress={onPress} className="flex-row items-center gap-3 pl-4 active:bg-accent">
      {leading}
      <View className={`min-h-[52px] flex-1 flex-row items-center gap-3 self-stretch pr-4 ${separator ? 'border-b border-border' : ''}`}>
        <View className="flex-1 gap-0.5 py-2">
          <Text numberOfLines={1} className="font-noto-medium text-[14px] leading-[19px]">
            {title}
          </Text>
          {subtitle ? (
            <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground">
              {subtitle}
            </Text>
          ) : null}
        </View>
        {trailing}
      </View>
    </Pressable>
  )
}
