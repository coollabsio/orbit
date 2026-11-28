import { useEffect } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'

const CHECK_INTERVAL_MS = 5 * 60_000

function showNewVersionToast() {
  toast('New version available', {
    id: 'orbit-new-version',
    duration: Infinity,
    dismissible: false,
    action: (
      <Button size="xs" className="ml-auto" onClick={() => window.location.reload()}>
        Reload
      </Button>
    ),
  })
}

/**
 * Compares the running build with the deployed one (`/orbit-build.json`, served with `no-store`) every few minutes
 * and when the tab becomes visible again, and asks the user to reload once they differ. The running build is the
 * `orbit-revision` meta tag that `vite build` adds to `index.html`; the dev server has none.
 */
export function NewVersionNotice() {
  useEffect(() => {
    const revision = document.querySelector<HTMLMetaElement>('meta[name="orbit-revision"]')?.content
    if (!revision) return

    let found = false
    const check = async () => {
      if (found || document.visibilityState !== 'visible') return
      try {
        const response = await fetch('/orbit-build.json', { cache: 'no-store' })
        if (!response.ok) return
        const deployed = (await response.json()) as { revision?: string }
        if (deployed.revision && deployed.revision !== revision) {
          found = true
          showNewVersionToast()
        }
      } catch {
        // Offline or the server is restarting; the next check tries again.
      }
    }
    // A lazy chunk of the old build is gone after a deploy.
    const preloadError = () => {
      found = true
      showNewVersionToast()
    }

    const interval = window.setInterval(check, CHECK_INTERVAL_MS)
    document.addEventListener('visibilitychange', check)
    window.addEventListener('vite:preloadError', preloadError)
    return () => {
      window.clearInterval(interval)
      document.removeEventListener('visibilitychange', check)
      window.removeEventListener('vite:preloadError', preloadError)
    }
  }, [])

  return null
}
