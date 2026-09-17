import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Logout, Setting, User } from 'reicon-react'
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
      className="user-menu-chevron"
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
    <svg className="user-menu-check" viewBox="0 0 12 12" fill="none" aria-hidden="true">
      <path d="m2.5 6.25 2.1 2.1 4.9-5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

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
    <Dropdown className="user-menu" direction="up" trigger={(open) => (
      <button
        type="button"
        className="user-menu-trigger"
        title={userName}
        aria-label={`Account menu for ${userName}`}
        aria-expanded={open}
        onClick={() => setAppearanceOpen(false)}
      >
        <span className="user-menu-avatar">{initial}</span>
        {!collapsed ? (
          <span className="user-menu-identity">
            <span className="user-menu-name">{userName}</span>
          </span>
        ) : null}
        {!collapsed ? <Chevron open={open} /> : null}
      </button>
    )}>
      {(close) => (
        <div className="user-menu-panel">
          <div className="user-menu-header">
            <div className="user-menu-header-name">{userName}</div>
            <div className="user-menu-header-email">{me?.email}</div>
          </div>
          <div className="listbox-separator" />
          <button
            type="button"
            className="listbox-option"
            onClick={() => {
              close()
              navigate('/profile')
            }}
          >
            <span className="user-menu-option-label">
              <User size={16} style={{ opacity: 0.8 }} />
              Account settings
            </span>
          </button>
          <button
            type="button"
            className="listbox-option"
            aria-expanded={appearanceOpen}
            onClick={() => setAppearanceOpen((o) => !o)}
          >
            <span className="user-menu-option-label">
              <Setting size={16} style={{ opacity: 0.8 }} />
              Appearance
            </span>
            <Chevron open={appearanceOpen} />
          </button>
          {appearanceOpen ? (
            <div className="user-menu-sub">
              {THEMES.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className="user-menu-sub-option"
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
          <div className="listbox-separator" />
          <button
            type="button"
            className="listbox-option"
            data-tone="danger"
            disabled={logout.isPending}
            onClick={() => logout.mutate(undefined, { onSuccess: () => navigate('/login', { replace: true }) })}
          >
            <span className="user-menu-option-label">
              <Logout size={16} style={{ opacity: 0.9 }} />
              {logout.isPending ? 'Logging out…' : 'Log out'}
            </span>
          </button>
          {logout.isError ? <p className="user-menu-error" role="alert">Could not log out. Please try again.</p> : null}
        </div>
      )}
    </Dropdown>
  )
}
