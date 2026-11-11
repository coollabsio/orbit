import { View } from 'react-native'

import { Button } from './Button'
import { OrbitLoader } from './OrbitLoader'
import { Text } from './Text'

/** A screen that loads: the pixel "O" loader, as on the web. */
export function LoadingView() {
  return (
    <View className="flex-1 items-center justify-center">
      <OrbitLoader />
    </View>
  )
}

export function ErrorView({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  return (
    <View className="flex-1 items-center justify-center gap-3 p-6">
      <Text className="text-center font-noto-semibold text-[17px] leading-[22px]">Could not load</Text>
      <Text className="text-center leading-[21px] text-muted-foreground">{error instanceof Error ? error.message : String(error)}</Text>
      <Button label="Retry" variant="outline" onPress={onRetry} />
    </View>
  )
}
