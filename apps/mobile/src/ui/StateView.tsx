import { View } from 'react-native'

import { Button } from './Button'
import { Spinner } from './Spinner'
import { Text } from './Text'

export function LoadingView() {
  return (
    <View className="flex-1 items-center justify-center">
      <Spinner size="large" />
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
