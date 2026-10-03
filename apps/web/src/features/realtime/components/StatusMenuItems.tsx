import { Edit, SmileCircle, Xmark } from 'reicon-react'
import type { Presence } from '@/api/generated/types.gen'
import { StatusMark } from '@/components/common/StatusMark'
import { DropdownMenuItem, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator } from '@/components/ui/dropdown-menu'
import { useSetStatus } from '@/features/auth/api'
import { useOwnStatus } from '@/features/realtime/useOwnStatus'

const PRESENCE_OPTIONS: { value: Presence; label: string; description?: string }[] = [
  { value: 'online', label: 'Online' },
  { value: 'idle', label: 'Idle' },
  { value: 'dnd', label: 'Do not disturb', description: 'No notifications' },
  { value: 'invisible', label: 'Invisible', description: 'You appear offline' },
]

/**
 * The signed-in user's status controls, as items of a `DropdownMenuContent`: the presence choice, then the custom
 * status. The dialog for the custom status belongs to the host (`onEditCustom`), because the menu closes on a click.
 */
export function StatusMenuItems({ onEditCustom }: { onEditCustom: () => void }) {
  const status = useOwnStatus()
  const setStatus = useSetStatus()
  const custom = { emoji: status.emoji, text: status.text, expires_at: status.expiresAt }
  const hasCustom = status.emoji !== null || status.text !== null

  return (
    <>
      <DropdownMenuRadioGroup aria-label="Status" value={status.presence} onValueChange={(value) => setStatus.mutate({ presence: value as Presence, ...custom })}>
        {PRESENCE_OPTIONS.map((option) => (
          <DropdownMenuRadioItem key={option.value} value={option.value} closeOnClick className="gap-2">
            <StatusMark status={option.value} size={10} className="bg-popover *:bg-popover" />
            <span className="flex min-w-0 flex-col">
              <span>{option.label}</span>
              {option.description ? <span className="text-xs text-muted-foreground">{option.description}</span> : null}
            </span>
          </DropdownMenuRadioItem>
        ))}
      </DropdownMenuRadioGroup>
      <DropdownMenuSeparator />
      <DropdownMenuItem onClick={onEditCustom}>
        {hasCustom ? <Edit /> : <SmileCircle />}
        {hasCustom ? 'Edit custom status' : 'Set custom status…'}
      </DropdownMenuItem>
      {hasCustom ? (
        <DropdownMenuItem onClick={() => setStatus.mutate({ presence: status.presence, emoji: null, text: null, expires_at: null })}>
          <Xmark />
          Clear custom status
        </DropdownMenuItem>
      ) : null}
    </>
  )
}
