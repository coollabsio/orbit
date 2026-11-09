import { useState } from 'react'
import { RefreshControl, type RefreshControlProps } from 'react-native'

import { useTheme } from '../theme'

/**
 * Pull-to-refresh that spins only for the user's own pull. Bound to `isRefetching`, a background refetch turned the
 * native spinner on without a pull, and iOS froze it when the screen lost focus mid-refetch.
 */
export function PullRefresh({ onRefresh, ...props }: Omit<RefreshControlProps, 'refreshing' | 'onRefresh'> & { onRefresh: () => Promise<unknown> }) {
  const { colors } = useTheme()
  const [refreshing, setRefreshing] = useState(false)
  const pull = () => {
    setRefreshing(true)
    void onRefresh().finally(() => setRefreshing(false))
  }
  return <RefreshControl tintColor={colors.mutedForeground} {...props} refreshing={refreshing} onRefresh={pull} />
}
