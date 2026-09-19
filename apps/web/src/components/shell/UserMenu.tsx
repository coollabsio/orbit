import { useState } from 'react'
import { useNavigate } from 'react-router'
import { LogOut, Settings, User } from 'lucide-react'
import { cn } from 'cn'
import { useTheme, type Theme } from '../../lib/themeContext'
import { Dropdown } from '../ui/Dropdown'
import { useCurrentUser, useLogout } from '../../features/auth/api'

const THEMES: { value: Theme; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
]

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      className="size-3.5 shrink-0 text-muted-foreground/70 transition-transform data-[open]:rotate-180"
      data-open={open || undefined}
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
    >
      <path d="M6 9l6 6 6-6" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

function Check() {
  return (
    <svg className="size-3.5 text-primary" viewBox="0 0 12 12" fill="none" aria-hidden="true">
      <path d="m2.5 6.25 2.1 2.1 4.9-5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

const optionClass =
  'flex min-h-11 w-full items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-accent hover:text-accent-foreground disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent data-[tone=danger]:text-destructive'

/** Signed-in profile and account actions shared by both sidebars. */
export function UserMenu({ collapsed = false }: { collapsed?: boolean }) {
  const user = useCurrentUser()
  const logout = useLogout()
  const navigate = useNavigate()
  const { theme, setTheme } = useTheme()
  const me = user.data
  const [appearanceOpen, setAppearanceOpen] = useState(false)

  const userName = me?.display_name ?? 'Account'
  const initial = (me?.display_name || me?.email || 'A').charAt(0).toUpperCase()

  return (
    <Dropdown className="w-[220px] max-w-[calc(100vw-32px)]" direction="up" trigger={(open) => (
      <button
        type="button"
        className={cn(
          'flex min-h-[34px] w-full min-w-0 flex-1 items-center gap-1.5 rounded-lg border border-border bg-card p-1.5 shadow-[0_1px_2px_rgba(0,0,0,0.05)] transition-colors hover:bg-muted',
          collapsed && 'w-8 px-[5px]',
        )}
        title={userName}
        aria-label={`Account menu for ${userName}`}
        aria-expanded={open}
        onClick={() => setAppearanceOpen(false)}
      >
        <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-[11px] font-semibold text-foreground">{initial}</span>
        {!collapsed ? (
          <span className="flex min-w-0 flex-1 text-left">
            <span className="min-w-0 truncate text-xs font-medium text-foreground">{userName}</span>
          </span>
        ) : null}
        {!collapsed ? <Chevron open={open} /> : null}
      </button>
    )}>
      {(close) => (
        <div className="p-1">
          <div className="min-w-0 px-2 py-1.5">
            <div className="truncate text-[13px] font-semibold text-foreground">{userName}</div>
            <div className="truncate text-[11px] text-muted-foreground/70">{me?.email}</div>
          </div>
          <div className="my-1 h-px bg-border" />
          <button
            type="button"
            className={optionClass}
            onClick={() => {
              close()
              navigate('/profile')
            }}
          >
            <span className="flex items-center gap-2">
              <User className="size-4 opacity-80" />
              Account settings
            </span>
          </button>
          <button
            type="button"
            className={optionClass}
            aria-expanded={appearanceOpen}
            onClick={() => setAppearanceOpen((o) => !o)}
          >
            <span className="flex items-center gap-2">
              <Settings className="size-4 opacity-80" />
              Appearance
            </span>
            <Chevron open={appearanceOpen} />
          </button>
          {appearanceOpen ? (
            <div className="mx-1 grid gap-0.5 pb-1 pl-6">
              {THEMES.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className="flex h-8 w-full items-center justify-between rounded-md px-2 text-left text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                  onClick={() => {
                    setTheme(option.value)
                    close()
                  }}
                >
                  <span>{option.label}</span>
                  {theme === option.value ? <Check /> : null}
                </button>
              ))}
            </div>
          ) : null}
          <div className="my-1 h-px bg-border" />
          <button
            type="button"
            className={optionClass}
            data-tone="danger"
            disabled={logout.isPending}
            onClick={() => logout.mutate(undefined, { onSuccess: () => navigate('/login', { replace: true }) })}
          >
            <span className="flex items-center gap-2">
              <LogOut className="size-4 opacity-90" />
              {logout.isPending ? 'Logging out…' : 'Log out'}
            </span>
          </button>
          {logout.isError ? <p className="px-2 py-1.5 text-xs text-destructive" role="alert">Could not log out. Please try again.</p> : null}
        </div>
      )}
    </Dropdown>
  )
}
