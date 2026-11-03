import { ActivityIndicator } from 'react-native'

import { useTheme } from '../theme'

export function Spinner({ size = 'small', color }: { size?: 'small' | 'large'; color?: string }) {
  const { colors } = useTheme()
  return <ActivityIndicator size={size} color={color ?? colors.mutedForeground} />
}
