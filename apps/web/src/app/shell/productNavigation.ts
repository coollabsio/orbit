export const primaryProductPath = '/tasks'

/** `VITE_HIDE_DOCS=1` hides Docs from the navigation (e.g. for tasks-only screenshots). */
export const docsHidden = import.meta.env.VITE_HIDE_DOCS === '1'

export const disabledProductPaths = ['/mail', '/chat', '/dm'] as const
export const coreSettingsPaths = ['/settings', '/settings/members', '/settings/sessions', '/settings/danger-zone', '/tasks-trash'] as const
export const mobileDockPaths = ['/tasks', '/docs', '/settings'] as const

export function isDisabledProductPath(pathname: string) {
  return disabledProductPaths.some((path) => pathname === path || pathname.startsWith(`${path}/`))
}
