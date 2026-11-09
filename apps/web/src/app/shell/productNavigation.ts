import { chatEnabled } from '@/lib/chatEnabled'

export { chatEnabled }

export const primaryProductPath = '/tasks'

/** `VITE_HIDE_DOCS=1` hides Docs from the navigation (e.g. for tasks-only screenshots). */
export const docsHidden = import.meta.env.VITE_HIDE_DOCS === '1'

/** Products that show as "Coming soon". */
export const disabledProductPathsFor = (chat: boolean): readonly string[] => (chat ? ['/mail'] : ['/mail', '/chat'])
/** The dock entries that are links; the others show disabled. Keep in sync with the sidebar's Workspace links. */
export const mobileDockPathsFor = (chat: boolean): readonly string[] => (chat ? ['/', '/tasks', '/docs', '/chat', '/settings'] : ['/', '/tasks', '/docs', '/settings'])

export const disabledProductPaths = disabledProductPathsFor(chatEnabled)
export const coreSettingsPaths = ['/settings', '/settings/members', '/settings/sessions', '/settings/danger-zone', '/tasks-trash'] as const
export const mobileDockPaths = mobileDockPathsFor(chatEnabled)

export function isDisabledProductPath(pathname: string) {
  return disabledProductPaths.some((path) => pathname === path || pathname.startsWith(`${path}/`))
}

/** Where a guest lands: guests are only in some chat channels. */
export const guestHomePath = '/chat'

/**
 * What a guest may open: Chat, the account settings that matter for chat (not Agentic, Keyboard shortcuts or
 * Storage, which are for tasks, docs, AI clients and offline media), and Admin (the installation's, which checks the
 * user itself). Every other page sends it to `guestHomePath`.
 */
export function guestMayOpen(pathname: string) {
  return /^\/(chat|admin)(\/|$)/.test(pathname) || /^\/profile(\/(general|security|sessions|appearance|notifications))?\/?$/.test(pathname)
}

/** Below `/chat` (a conversation, a thread, Unreads, Threads): a full screen on a phone, so the dock is hidden. */
export function isChatConversationPath(pathname: string) {
  return /^\/chat\/[^/]/.test(pathname)
}
