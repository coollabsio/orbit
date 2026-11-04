import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Lock } from 'reicon-react'
import { PAYMENT_REQUIRED_EVENT } from '@/api/client'
import { useWorkspaceBilling } from '@/features/workspaces/billing'

const DAY_MILLIS = 24 * 60 * 60 * 1000

/**
 * Orbit Cloud: one line above the top bar while the workspace is on its trial or has no subscription that grants
 * access (its data stays readable). The link opens the billing page. A refused write fetches the billing state again,
 * so the line appears as soon as a subscription lapses. The billing state is only served to members.
 */
export function ReadOnlyBanner() {
  const billing = useWorkspaceBilling()
  const { refetch } = billing
  const [openedAt] = useState(Date.now)

  useEffect(() => {
    const refused = () => void refetch()
    window.addEventListener(PAYMENT_REQUIRED_EVENT, refused)
    return () => window.removeEventListener(PAYMENT_REQUIRED_EVENT, refused)
  }, [refetch])

  const data = billing.data
  if (data?.access === 'trial' && data.trial) {
    const days = Math.max(1, Math.ceil((Date.parse(data.trial.ends_at) - openedAt) / DAY_MILLIS))
    return (
      <div
        data-slot="trial-banner"
        role="status"
        className="flex min-h-8 shrink-0 items-center justify-center gap-2 border-b border-destructive/20 bg-destructive/10 px-3 py-1 text-xs text-destructive dark:bg-destructive/20"
      >
        <span className="min-w-0 truncate font-medium">
          Trial: {days} {days === 1 ? 'day' : 'days'} left
        </span>
        <Link to="/settings/billing" className="shrink-0 font-semibold text-destructive underline underline-offset-4">
          {data.can_manage ? 'Choose a plan' : 'View billing'}
        </Link>
      </div>
    )
  }
  if (data?.access !== 'read_only') return null
  return (
    <div
      data-slot="read-only-banner"
      role="status"
      className="flex min-h-8 shrink-0 items-center justify-center gap-2 border-b border-border bg-muted px-3 py-1 text-xs text-muted-foreground"
    >
      <Lock className="size-3.5 shrink-0" />
      <span className="min-w-0 truncate">This workspace is read-only because it has no active subscription.</span>
      <Link to="/settings/billing" className="shrink-0 font-medium text-foreground underline-offset-4 hover:underline">
        {data.can_manage ? 'Choose a plan' : 'View billing'}
      </Link>
    </div>
  )
}
