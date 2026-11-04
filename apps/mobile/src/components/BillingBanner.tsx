import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { View } from 'react-native'

import { billingBanner, billingKey, workspaceBilling } from '../data/billing'
import { useAuthOptions } from '../hooks/account'
import { useOrbit } from '../orbit-context'
import { useTheme } from '../theme'
import { Icon, Text } from '../ui'

/**
 * The web's `ReadOnlyBanner` (Orbit Cloud): the trial's days left, or the read-only line, above the dock on every tab.
 * Information only: store rules forbid steering to purchases outside the app, so it has no link and names no plan or price.
 */
export function BillingBanner() {
  const { client, workspaceId } = useOrbit()
  const { colors } = useTheme()
  const enabled = useAuthOptions().data?.billing_enabled ?? false
  const billing = useQuery({ queryKey: billingKey(workspaceId), queryFn: () => workspaceBilling(client, workspaceId), enabled })
  const [openedAt] = useState(Date.now)
  const banner = billingBanner(billing.data, openedAt)
  if (!banner) return null
  const trial = banner.kind === 'trial'
  return (
    <View
      accessibilityRole="summary"
      className={`min-h-8 flex-row items-center justify-center gap-2 border-t px-3 py-1 ${trial ? 'border-destructive/20 bg-destructive/10 dark:bg-destructive/20' : 'border-border bg-muted'}`}
    >
      {!trial ? <Icon name="lock" size={14} color={colors.mutedForeground} /> : null}
      <Text numberOfLines={1} className={`shrink text-[12px] ${trial ? 'font-noto-medium text-destructive' : 'text-muted-foreground'}`}>
        {banner.text}
      </Text>
    </View>
  )
}
