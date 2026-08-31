import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { Logout, Profile, Setting } from 'reicon-react'
import { useTheme, type Theme } from '../../lib/themeContext'
import { useAppState } from '../../mock/store'

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

/** Coolify `x-top-user-menu sidebar`: account pill that opens upward with Profile, Appearance, Log out. */
export function UserMenu({ collapsed = false }: { collapsed?: boolean }) {
  const state = useAppState()
  const navigate = useNavigate()
  const { theme, setTheme } = useTheme()
  const me = state.users.find((u) => u.id === state.currentUserId)
  const [open, setOpen] = useState(false)
  const [appearanceOpen, setAppearanceOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)

  const userName = me?.name ?? 'Account'
  const initial = (me?.name || me?.email || 'A').charAt(0).toUpperCase()

  useEffect(() => {
    if (!open) return
    const onPointerDown = (e: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false)
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open])

  const toggle = () => {
    setAppearanceOpen(false)
    setOpen((o) => !o)
  }

  return (
    <div ref={rootRef} className="user-menu">
      <button
        type="button"
        className="user-menu-trigger"
        title={userName}
        aria-label={`Account menu for ${userName}`}
        aria-expanded={open}
        onClick={toggle}
      >
        <span className="user-menu-avatar">{initial}</span>
        {!collapsed ? <span className="user-menu-name">{userName}</span> : null}
        {!collapsed ? <Chevron open={open} /> : null}
      </button>

      {open ? (
        <div className="listbox-panel user-menu-panel">
          <div className="user-menu-header">
            <div className="user-menu-header-name">{userName}</div>
            <div className="user-menu-header-email">{me?.email}</div>
          </div>
          <div className="listbox-separator" />
          <button
            type="button"
            className="listbox-option"
            onClick={() => {
              setOpen(false)
              navigate('/profile')
            }}
          >
            <span className="user-menu-option-label">
              <Profile size={16} style={{ opacity: 0.8 }} />
              Profile
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
                    setOpen(false)
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
            disabled
            title="Sign-in is not connected in this preview."
          >
            <span className="user-menu-option-label">
              <Logout size={16} style={{ opacity: 0.9 }} />
              Log out
            </span>
          </button>
        </div>
      ) : null}
    </div>
  )
}
