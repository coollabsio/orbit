/** What a member shows as; `invisible` is only ever the signed-in user's own choice. */
export type MemberStatus = 'online' | 'idle' | 'dnd' | 'offline' | 'invisible'

const LABELS: Record<MemberStatus, string> = {
  online: 'Online',
  idle: 'Idle',
  dnd: 'Do not disturb',
  offline: 'Offline',
  invisible: 'Invisible',
}

export function statusLabel(status: MemberStatus): string {
  return LABELS[status]
}
