import { useState } from 'react'
import { Pressable, View } from 'react-native'

import { useTheme } from '../theme'
import { Icon } from './Icon'
import { Text } from './Text'

/**
 * A label with help text: the web shows the text in an `InfoTip` tooltip; here a tap on the info icon shows it under
 * the label. `className` styles the label text; `required` adds the pink `*`.
 */
export function LabelTip({ label, text, required = false, className = 'font-noto-medium text-[14px] leading-[19.25px]' }: { label: string; text: string; required?: boolean; className?: string }) {
  const { colors } = useTheme()
  const [open, setOpen] = useState(false)
  return (
    <View className="min-w-0 gap-0.5">
      <View className="flex-row flex-wrap items-center gap-1.5">
        <Text className={className}>
          {label}
          {required ? <Text className="font-noto-semibold text-primary">{'  *'}</Text> : null}
        </Text>
        <Pressable accessibilityRole="button" accessibilityLabel={`About ${label}`} accessibilityHint={text} hitSlop={10} onPress={() => setOpen((value) => !value)}>
          <Icon name="infoCircle" size={14} color={colors.mutedForeground} />
        </Pressable>
      </View>
      {open ? <Text className="text-[12px] leading-[17px] text-muted-foreground">{text}</Text> : null}
    </View>
  )
}
