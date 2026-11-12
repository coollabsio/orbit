import { Stack } from 'expo-router'

import { useTheme } from '../../theme'

/** The pull request screens: a full-screen modal over the current tab (the root layout presents it), and a file's diff
 *  pushes inside it. */
export default function PullsLayout() {
  const { colors } = useTheme()
  return <Stack screenOptions={{ headerShown: false, contentStyle: { backgroundColor: colors.background } }} />
}
