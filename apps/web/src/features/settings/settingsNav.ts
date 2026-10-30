import { useContext, type ComponentType } from 'react'
import { ArchiveBox, Brush, Card, Chart, ClipboardText, CloudConnection, Cpu, SecurityUser, Driver, Key, Keyboard, Link2, Lock, Login, Notification as Bell, Setting2 as Settings, ShieldTick as ShieldCheck, SmileCircle, Sms as Mail, Sticker, Danger as TriangleAlert, DocumentDownload, User, People as Users } from 'reicon-react'
import type { Permission } from '@orbit/api/generated/types.gen'
import { RobotIcon } from '@/components/common/icons/RobotIcon'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { useBillingEnabled } from '@/features/workspaces/billing'

export interface SettingsNavItem {
  to: string
  label: string
  icon: ComponentType<{ className?: string }>
  end?: boolean
  /** Shown only to roles that hold this permission. */
  permission?: Permission
  /** Shown only in Orbit Cloud (billing on). */
  cloud?: boolean
}

export interface SettingsNavSection {
  label: string
  items: SettingsNavItem[]
}

export type SettingsAreaName = 'settings' | 'profile' | 'admin'

interface SettingsArea {
  title: string
  icon: SettingsNavItem['icon']
  sections: SettingsNavSection[]
}

const AREAS: Record<SettingsAreaName, SettingsArea> = {
  /** The workspace's settings. */
  settings: {
    title: 'Settings',
    icon: Settings,
    sections: [
      {
        label: 'Workspace',
        items: [
          { to: '/settings', label: 'General', icon: Settings, end: true },
          { to: '/settings/members', label: 'Members', icon: Users },
          { to: '/settings/billing', label: 'Billing', icon: Card, cloud: true },
        ],
      },
      {
        label: 'Chat',
        items: [
          { to: '/settings/emoji', label: 'Emoji', icon: SmileCircle, permission: 'chat.manage' },
          { to: '/settings/stickers', label: 'Stickers', icon: Sticker, permission: 'chat.manage' },
        ],
      },
      {
        label: 'Integrations',
        items: [
          { to: '/settings/file-storage', label: 'File storage', icon: CloudConnection, permission: 'storage.manage' },
          { to: '/settings/github', label: 'GitHub', icon: Link2 },
          { to: '/settings/api-tokens', label: 'API tokens', icon: Key, permission: 'api_tokens.manage' },
        ],
      },
      {
        label: 'Advanced',
        items: [
          { to: '/settings/export', label: 'Export', icon: DocumentDownload, permission: 'workspace.export' },
          { to: '/settings/danger-zone', label: 'Danger zone', icon: TriangleAlert, permission: 'workspace.delete' },
        ],
      },
    ],
  },
  /** What belongs to the signed-in person, in every workspace. */
  profile: {
    title: 'Account settings',
    icon: User,
    sections: [
      {
        label: 'Account',
        items: [
          { to: '/profile', label: 'Profile', icon: User, end: true },
          { to: '/profile/appearance', label: 'Appearance', icon: Brush },
          { to: '/profile/notifications', label: 'Notifications', icon: Bell },
          { to: '/profile/security', label: 'Security', icon: Lock },
          { to: '/profile/sessions', label: 'Sessions', icon: ShieldCheck },
          { to: '/profile/agentic', label: 'Agentic', icon: RobotIcon },
          { to: '/profile/shortcuts', label: 'Keyboard shortcuts', icon: Keyboard },
          { to: '/profile/storage', label: 'Storage', icon: Driver },
        ],
      },
    ],
  },
  /** What belongs to the whole Orbit instance. Only the root user (the account created at setup) sees it. */
  admin: {
    title: 'Admin',
    icon: SecurityUser,
    sections: [
      {
        label: 'Instance',
        items: [
          { to: '/admin', label: 'General', icon: Settings, end: true },
          { to: '/admin/users', label: 'Users', icon: Users },
          { to: '/admin/email', label: 'Email', icon: Mail },
          { to: '/admin/sign-in-providers', label: 'Sign-in providers', icon: Login },
          { to: '/admin/limits', label: 'Limits', icon: Chart },
        ],
      },
      {
        label: 'Data',
        items: [
          { to: '/admin/storage', label: 'Storage', icon: CloudConnection },
          { to: '/admin/backups', label: 'Backups', icon: ArchiveBox },
        ],
      },
      {
        label: 'Monitoring',
        items: [
          { to: '/admin/audit', label: 'Audit log', icon: ClipboardText },
          { to: '/admin/jobs', label: 'Background jobs', icon: Cpu },
        ],
      },
    ],
  },
}

const under = (pathname: string, root: string) => pathname === root || pathname.startsWith(`${root}/`)

/** The settings area a route belongs to; the sidebar shows that area's links instead of the app links. */
export function settingsAreaOf(pathname: string): SettingsAreaName | null {
  if (under(pathname, '/settings')) return 'settings'
  if (under(pathname, '/profile')) return 'profile'
  if (under(pathname, '/admin')) return 'admin'
  return null
}

/** An area's title, icon and the links this person may open. */
export function useSettingsArea(name: SettingsAreaName): SettingsArea {
  const permissions = useContext(WorkspaceContext)?.workspace.permissions ?? []
  const cloud = useBillingEnabled()
  const area = AREAS[name]
  const sections = area.sections
    .map((section) => ({
      ...section,
      items: section.items.filter((item) => (!item.permission || permissions.includes(item.permission)) && (!item.cloud || cloud)),
    }))
    .filter((section) => section.items.length > 0)
  return { ...area, sections }
}
