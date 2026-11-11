import type { ReactNode } from 'react'
import { Navigate, Outlet } from 'react-router'
import { useAuthOptions } from '@/features/auth/api'

/** Mail's pages, only while Mail is on for the server; otherwise `fallback`. Nothing shows until the options load. */
export function MailGate({ fallback, children }: { fallback: string; children?: ReactNode }) {
  const options = useAuthOptions()
  if (!options.data) return null
  if (!options.data.mail_enabled) return <Navigate to={fallback} replace />
  return children ?? <Outlet />
}
