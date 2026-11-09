import { Navigate, Outlet, useLocation } from 'react-router'
import { guestHomePath, guestMayOpen } from '@/app/shell/productNavigation'
import { useIsGuest } from './permissions'

/** A guest only opens Chat and its own account settings; any other page of the workspace sends it to Chat. */
export function GuestWorkspaceGate() {
  const guest = useIsGuest()
  const { pathname } = useLocation()
  return guest && !guestMayOpen(pathname) ? <Navigate to={guestHomePath} replace /> : <Outlet />
}
