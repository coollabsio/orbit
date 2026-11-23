import { View } from 'react-native'

import { useTheme } from '../theme'
import { HelpTip } from './HelpTip'
import { Icon } from './Icon'
import { Text } from './Text'

/**
 * A label with help text, as the web `InfoTip`: a tap on the info icon shows the text in a tooltip. `className` styles
 * the label text; `required` adds the pink `*`.
 */
export function LabelTip({ label, text, required = false, className = 'font-noto-medium text-[14px] leading-[19.25px]' }: { label: string; text: string; required?: boolean; className?: string }) {
  const { colors } = useTheme()
  return (
    <View className="min-w-0 flex-row flex-wrap items-center gap-1.5">
      <Text className={className}>
        {label}
        {required ? <Text className="font-noto-semibold text-primary">{'  *'}</Text> : null}
      </Text>
      <HelpTip text={text} label={`About ${label}`}>
        <Icon name="infoCircle" size={14} color={colors.mutedForeground} />
      </HelpTip>
    </View>
  )
}
