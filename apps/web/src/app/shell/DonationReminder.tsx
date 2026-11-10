import { useState } from 'react'
import { Heart, Xmark } from 'reicon-react'
import { buttonVariants } from '@/components/ui/button'
import { useAuthOptions } from '@/features/auth/api'
import { useIsInstallationAdmin } from '@/features/workspaces/permissions'

const STORAGE_KEY = 'orbit:donation_reminder_dismissed'
const SPONSOR_URL = 'https://github.com/sponsors/coollabsio'
const OTHER_WAYS_URL = 'https://coollabs.io/sponsorships?ref=orbit'

/** Like Coolify: a dismissal holds for the rest of its calendar month, so the reminder comes back once a month. */
export function shouldShowDonationReminder(dismissedAt: string | null, now: Date): boolean {
  const time = Number(dismissedAt)
  if (!dismissedAt || !Number.isFinite(time)) return true
  const dismissed = new Date(time)
  return dismissed.getFullYear() !== now.getFullYear() || dismissed.getMonth() !== now.getMonth()
}

function readDismissedAt(): string | null {
  try {
    return window.localStorage.getItem(STORAGE_KEY)
  } catch {
    return null
  }
}

/**
 * Self-hosted Orbit only (no billing): a card in the bottom corner asks installation admins for a donation, once a
 * month. Any choice (donate, "Maybe next time", close) hides it until next month in this browser.
 */
export function DonationReminder() {
  const options = useAuthOptions().data
  const admin = useIsInstallationAdmin()
  const [open, setOpen] = useState(() => shouldShowDonationReminder(readDismissedAt(), new Date()))
  if (!open || !admin || !options || options.billing_enabled) return null

  const dismiss = () => {
    try {
      window.localStorage.setItem(STORAGE_KEY, String(Date.now()))
    } catch {
      // Private mode: the reminder only closes for this page load.
    }
    setOpen(false)
  }

  return (
    <section
      data-slot="donation-reminder"
      aria-label="Support Orbit"
      className="fixed right-4 bottom-4 z-40 w-[min(380px,calc(100vw-2rem))] rounded-xl border bg-popover p-4 text-popover-foreground shadow-lg animate-in fade-in-0 slide-in-from-bottom-2 duration-200 ease-out motion-reduce:animate-none max-[899px]:bottom-[calc(5rem+env(safe-area-inset-bottom,0px))]"
    >
      <button
        type="button"
        aria-label="Dismiss donation reminder"
        onClick={dismiss}
        className="absolute top-2.5 right-2.5 flex size-7 items-center justify-center rounded-md text-muted-foreground transition-[color,background-color] hover:bg-muted hover:text-foreground"
      >
        <Xmark className="size-4" />
      </button>
      <div className="flex items-start gap-3 pr-7">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <Heart className="size-5" />
        </span>
        <div className="min-w-0">
          <h2 className="text-sm font-semibold">Enjoy Orbit? Support our work.</h2>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            Orbit will be free forever. To support our work, please consider a donation.
          </p>
        </div>
      </div>
      <div className="mt-4 flex items-center gap-2">
        <a href={SPONSOR_URL} target="_blank" rel="noopener noreferrer" onClick={dismiss} className={buttonVariants({ size: 'sm' })}>
          GitHub Sponsors
        </a>
        <a href={OTHER_WAYS_URL} target="_blank" rel="noopener noreferrer" onClick={dismiss} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
          Other ways
        </a>
        <button type="button" onClick={dismiss} className="ml-auto px-1 text-xs font-medium whitespace-nowrap text-muted-foreground transition-colors hover:text-foreground">
          Maybe next time
        </button>
      </div>
    </section>
  )
}
