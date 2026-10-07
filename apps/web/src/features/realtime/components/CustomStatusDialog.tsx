import { useId, useState } from 'react'
import { SmileCircle } from 'reicon-react'
import { Emoji } from '@/components/common/Emoji'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { Modal } from '@/components/common/Modal'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { DialogFooter } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useSetStatus } from '@/features/auth/api'
import { useOwnStatus } from '@/features/realtime/useOwnStatus'

type ClearAfter = 'keep' | 'never' | '30m' | '1h' | '4h' | 'today'

const CLEAR_AFTER_OPTIONS: { value: ClearAfter; label: string }[] = [
  { value: 'never', label: "Don't clear" },
  { value: '30m', label: '30 minutes' },
  { value: '1h', label: '1 hour' },
  { value: '4h', label: '4 hours' },
  { value: 'today', label: 'Today' },
]

const MINUTES: Partial<Record<ClearAfter, number>> = { '30m': 30, '1h': 60, '4h': 240 }

/** When the custom status ends; "Today" is the end of the local day. `keep` leaves the end that is set. */
function clearAfterTime(choice: ClearAfter, current: string | null, now = new Date()): string | null {
  if (choice === 'keep') return current
  if (choice === 'never') return null
  if (choice === 'today') return new Date(now.getFullYear(), now.getMonth(), now.getDate(), 23, 59, 59, 999).toISOString()
  return new Date(now.getTime() + (MINUTES[choice] ?? 0) * 60_000).toISOString()
}

/**
 * Sets or clears the signed-in user's custom status (emoji, text and when it ends). The presence stays as it is.
 * Callers mount it while it is open.
 */
export function CustomStatusDialog({ onClose }: { onClose: () => void }) {
  const status = useOwnStatus()
  const setStatus = useSetStatus()
  const [emoji, setEmoji] = useState(status.emoji)
  const [text, setText] = useState(status.text ?? '')
  // A status that already has an end keeps it unless the user picks another one.
  const [options] = useState(() =>
    status.expiresAt
      ? [{ value: 'keep' as const, label: `Keep (${new Date(status.expiresAt).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })})` }, ...CLEAR_AFTER_OPTIONS]
      : CLEAR_AFTER_OPTIONS,
  )
  const [clearAfter, setClearAfter] = useState<ClearAfter>(options[0].value)
  const [emojiOpen, setEmojiOpen] = useState(false)
  const textId = useId()
  const hadCustom = status.emoji !== null || status.text !== null

  const save = (custom: { emoji: string | null; text: string | null; expires_at: string | null }) => {
    if (setStatus.isPending) return
    // a failure keeps the dialog and what was typed; the mutation says what went wrong
    setStatus.mutate({ presence: status.presence, ...custom }, { onSuccess: onClose })
  }

  const submit = (event: React.FormEvent) => {
    event.preventDefault()
    const trimmed = text.trim() || null
    const empty = emoji === null && trimmed === null
    save({ emoji, text: trimmed, expires_at: empty ? null : clearAfterTime(clearAfter, status.expiresAt) })
  }

  return (
    <Modal title="Custom status" onClose={onClose} className="sm:max-w-md" dismissible={!setStatus.isPending}>
      <form className="grid gap-4" onSubmit={submit}>
        <Field>
          <FieldLabel htmlFor={textId}>Status</FieldLabel>
          <div className="flex items-center gap-2">
            <Popover open={emojiOpen} onOpenChange={setEmojiOpen} modal={false}>
              <Tip label={emoji ? 'Change emoji' : 'Add emoji'}>
                <PopoverTrigger render={<Button type="button" variant="outline" size="icon" aria-label={emoji ? 'Change emoji' : 'Add emoji'} />}>
                  {emoji ? <Emoji value={emoji} /> : <SmileCircle className="text-muted-foreground" />}
                </PopoverTrigger>
              </Tip>
              <PopoverContent align="start" className="w-auto gap-0 p-0">
                <EmojiPicker
                  onPick={(picked) => {
                    setEmoji(picked)
                    setEmojiOpen(false)
                  }}
                  onRemove={
                    emoji
                      ? () => {
                          setEmoji(null)
                          setEmojiOpen(false)
                        }
                      : undefined
                  }
                />
              </PopoverContent>
            </Popover>
            <Input
              id={textId}
              autoFocus
              className="min-w-0 flex-1"
              maxLength={100}
              placeholder="What's your status?"
              value={text}
              onChange={(event) => setText(event.target.value)}
            />
          </div>
        </Field>
        <Field>
          <FieldLabel>Clear after</FieldLabel>
          <Select items={options} value={clearAfter} onValueChange={(value) => setClearAfter(value as ClearAfter)}>
            <SelectTrigger aria-label="Clear after" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {options.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <DialogFooter>
          <Button type="button" variant="outline" disabled={!hadCustom || setStatus.isPending} onClick={() => save({ emoji: null, text: null, expires_at: null })}>
            Clear status
          </Button>
          <Button type="submit" disabled={setStatus.isPending || (!hadCustom && emoji === null && !text.trim())}>
            Save
          </Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
