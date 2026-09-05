import { useLocation } from 'react-router'
import { isMockBackedPath } from './mockFeatures'

export function MockFeatureBadge() {
  const { pathname } = useLocation()
  if (!isMockBackedPath(pathname)) return null

  return (
    <span className="mock-feature-badge" title="This screen uses local mock data and does not persist.">
      Mock data
    </span>
  )
}
