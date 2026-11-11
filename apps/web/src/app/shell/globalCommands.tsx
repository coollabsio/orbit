import { useNavigate } from 'react-router'
import { useTheme } from '@/lib/themeContext'
import type { CommandId } from '@/shortcuts/commands'
import { useCommand } from '@/shortcuts/useCommand'
import { useIsGuest } from '@/features/workspaces/permissions'
import { useMailEnabled } from '@/features/mail/enabled'
import { chatEnabled, docsHidden, guestMayOpen } from './productNavigation'

/** Where each navigation command goes. */
export const NAVIGATION: ReadonlyArray<{ id: CommandId; to: string }> = [
  { id: 'nav.inbox', to: '/activity' },
  { id: 'nav.tasks', to: '/tasks' },
  { id: 'nav.mine', to: '/tasks?view=mine' },
  { id: 'nav.week', to: '/tasks?view=current_week' },
  { id: 'nav.overdue', to: '/tasks?view=overdue' },
  { id: 'nav.views', to: '/views' },
  { id: 'nav.docs', to: '/docs' },
  { id: 'nav.chat', to: '/chat' },
  { id: 'nav.mail', to: '/mail' },
  { id: 'nav.trash', to: '/tasks-trash' },
  { id: 'nav.settings', to: '/settings' },
  { id: 'nav.profile', to: '/profile' },
]

function NavigationCommand({ id, to }: { id: CommandId; to: string }) {
  const navigate = useNavigate()
  useCommand(id, () => navigate(to))
  return null
}

/** Mounts the navigation commands; one component each, so the list can change without breaking the rules of hooks. */
export function NavigationCommands() {
  const guest = useIsGuest()
  const mail = useMailEnabled()
  return NAVIGATION.filter(({ id, to }) => !(docsHidden && id === 'nav.docs') && !(id === 'nav.chat' && !chatEnabled) && !(id === 'nav.mail' && !mail) && !(guest && !guestMayOpen(to))).map((entry) => <NavigationCommand key={entry.id} {...entry} />)
}

/** The commands that work on every page of the app shell. */
export function useGlobalCommands({ openPalette, openHelp }: { openPalette: () => void; openHelp: () => void }) {
  const navigate = useNavigate()
  const { toggleTheme } = useTheme()
  // a guest has no workspace settings
  const guest = useIsGuest()
  useCommand('search.open', openPalette)
  useCommand('help.open', openHelp)
  useCommand('settings.open', () => navigate(guest ? '/profile' : '/settings'))
  useCommand('theme.toggle', toggleTheme)
}
