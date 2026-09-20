import { useLocation } from 'react-router'
import { isMockBackedPath } from './mockFeatures'

export function MockFeatureBadge() {
  const { pathname } = useLocation()
  if (!isMockBackedPath(pathname)) return null

  return (
    <span
      className="inline-flex w-fit rounded-full border border-border px-1.5 py-0.5 text-[10px] font-semibold tracking-[0.02em] text-muted-foreground/70"
      title="This screen uses local mock data and does not persist."
    >
      Mock data
    </span>
  )
}
