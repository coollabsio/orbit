import { View } from 'react-native'

import { Text } from './Text'

/** The web's pink count pill: 16px tall, `99+` beyond 99. */
export function CountBadge({ count }: { count: number }) {
  return (
    <View className="h-4 min-w-4 items-center justify-center rounded-md bg-primary px-1">
      <Text className="font-noto-semibold text-[10px] leading-[12px] text-primary-foreground">{count > 99 ? '99+' : count}</Text>
    </View>
  )
}
