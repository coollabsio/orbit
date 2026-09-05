export const primaryProductPath = '/tasks'

export const disabledProductPaths = ['/docs', '/mail', '/chat', '/dm', '/inbox', '/profile'] as const
export const coreSettingsPaths = ['/settings', '/settings/members', '/settings/invitations', '/settings/sessions', '/tasks-trash'] as const
export const mobileDockPaths = ['/tasks', '/settings'] as const

export function isDisabledProductPath(pathname: string) {
  return disabledProductPaths.some((path) => pathname === path || pathname.startsWith(`${path}/`))
}
