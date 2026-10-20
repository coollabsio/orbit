import { Link, Outlet } from 'react-router'
import { Add } from 'reicon-react'
import { cn } from 'cn'
import { buttonVariants } from '@/components/ui/button'
import { UserMenu } from './UserMenu'

/**
 * The app of an operator (the root user or an instance admin) who has no workspace: operators are not customers, so
 * they work in Admin and their account settings. "Create workspace" still opens the onboarding.
 */
export function OperatorShell() {
  return (
    <div className="flex h-[var(--app-height,100svh)] w-full flex-col overflow-hidden bg-background pt-[env(safe-area-inset-top,0px)] pl-[env(safe-area-inset-left,0px)] pr-[env(safe-area-inset-right,0px)]">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-border px-4">
        {/* 22px keeps the 11-unit pixel logo on whole device pixels. */}
        <Link to="/admin" className="flex items-center gap-2 text-[13px] font-bold tracking-[0.08em] text-primary uppercase">
          <img src="/logo.svg" alt="" className="size-[22px]" />
          Orbit
        </Link>
        <div className="ml-auto flex min-w-0 items-center gap-2">
          <Link to="/onboarding" className={cn(buttonVariants({ variant: 'outline' }), 'gap-1.5')}>
            <Add className="size-4" />
            Create workspace
          </Link>
          <div className="w-44 min-w-0 max-sm:w-auto">
            <UserMenu />
          </div>
        </div>
      </header>
      <div className="flex min-h-0 flex-1">
        <Outlet />
      </div>
    </div>
  )
}
