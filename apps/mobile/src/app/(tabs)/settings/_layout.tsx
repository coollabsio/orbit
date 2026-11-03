import { Stack } from 'expo-router'

import { useTheme } from '../../../theme'

export default function Layout() {
  const { colors } = useTheme()
  // The settings and admin pages switch like tabs on the web (the grid at the top), so they appear without a slide.
  return <Stack screenOptions={{ headerShown: false, animation: 'none', contentStyle: { backgroundColor: colors.background } }} />
}
