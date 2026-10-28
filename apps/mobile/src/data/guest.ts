/**
 * Guests: outside people who are only in some private chat channels. The server refuses them everything else; the app
 * shows them only Chat and their account settings (the web's `guestMayOpen`).
 */

/** Where a guest lands. */
export const guestHome = '/chat'

/**
 * What a guest may open: Chat, the Settings tab's own page (account links, leave the workspace), the account settings
 * and Admin (the installation's, which checks the user itself).
 */
export function guestMayOpen(pathname: string): boolean {
  return pathname === '/settings' || /^\/(chat|settings\/(profile|appearance|notifications|security|sessions|admin))(\/|$)/.test(pathname)
}

export const isGuestRole = (role: string | undefined) => role === 'guest'
