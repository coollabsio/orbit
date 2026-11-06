import { useState } from 'react'
import { OrbitLoader } from './OrbitUnavailable'

/**
 * The first load: AuthGate, WorkspaceProvider and BillingReady each show this screen in turn, and index.html shows
 * the same loader before the app starts. Its fade-in (after 400ms, so fast loads never flash it) runs on the page's
 * clock, not per mount, so on a slow connection the loader comes in once and stays, instead of again at each stage.
 */
export function LoadingScreen() {
  const [fadeDelay] = useState(() => 400 - performance.now())
  return (
    <main className="grid min-h-full place-items-center bg-background">
      <div role="status" aria-label="Loading" className="duration-200 animate-in fade-in fill-mode-both" style={{ animationDelay: `${fadeDelay}ms` }}>
        <OrbitLoader />
      </div>
    </main>
  )
}
