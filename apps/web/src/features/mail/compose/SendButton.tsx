// Send, with a chevron for Send later (U3): "Tomorrow 8:00", "Monday 8:00" and "Pick date and time", in the user's
// time zone; "Send and archive" in a thread. Off with the reason in the tooltip (aria-disabled, so the reason stays
// reachable by hover and focus).
import { useMemo, useRef, useState } from 'react'
import { ChevronDown, Clock, Send } from 'reicon-react'
import { cn } from 'cn'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { Spinner } from '@/components/ui/spinner'
import { ShortcutKeys } from '@/shortcuts/Shortcut'
import { dayIn, sendLaterPresets, sendTimeLabel, SEND_LATER_HOUR, zonedDateTime, type CalendarDay } from './composeLib'
import type { SendOptions } from './useComposerSend'
import { useMailTimeZone } from './useMailTimeZone'

interface SendButtonProps {
  /** Why Send is off, or `null`. */
  blocked: string | null
  sending: boolean
  /** The pre-send checks are showing: the button sends anyway. */
  anyway: boolean
  /** The draft answers a thread: "Send and archive" is offered. */
  inThread: boolean
  onSend: (options?: SendOptions) => void
}

export function SendButton({ blocked, sending, anyway, inThread, onSend }: SendButtonProps) {
  const timeZone = useMailTimeZone()
  const [menuOpen, setMenuOpen] = useState(false)
  const [picking, setPicking] = useState(false)
  const group = useRef<HTMLDivElement>(null)
  // read when the menu opens: "Tomorrow" is tomorrow at that moment
  const presets = useMemo(() => (menuOpen ? sendLaterPresets(new Date(), timeZone) : []), [menuOpen, timeZone])
  const off = blocked !== null || sending

  return (
    <div ref={group} data-slot="send-button" className="flex shrink-0 items-center">
      <Tip
        label={
          blocked ?? (
            <span className="flex items-center gap-2">
              {anyway ? 'Send anyway' : 'Send'} <ShortcutKeys keys="Mod+Enter" />
            </span>
          )
        }
      >
        <Button
          data-slot="composer-send"
          aria-disabled={off || undefined}
          className={cn(
            // Send press: scale 0.97 on the button's own 120 ms transition (the Flat style's press)
            'min-w-20 rounded-r-none active:not-aria-[haspopup]:scale-[0.97]',
            off && 'cursor-not-allowed opacity-50 hover:translate-y-0 hover:bg-primary',
          )}
          onClick={() => onSend()}
        >
          {sending ? <Spinner className="size-3.5" /> : <Send />}
          {anyway ? 'Send anyway' : 'Send'}
        </Button>
      </Tip>
      {off ? (
        <Tip label={blocked ?? 'Sending…'}>
          <Button aria-label="Send later" aria-disabled className="cursor-not-allowed rounded-l-none border-l border-l-primary-foreground/20 px-1.5 opacity-50 hover:translate-y-0 hover:bg-primary">
            <ChevronDown />
          </Button>
        </Tip>
      ) : (
        <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
          <Tip label="Send later">
            <DropdownMenuTrigger render={<Button aria-label="Send later" className="rounded-l-none border-l border-l-primary-foreground/20 px-1.5" />}>
              <ChevronDown />
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent side="top" align="start" className="w-64">
            <DropdownMenuGroup>
              <DropdownMenuLabel>Send later</DropdownMenuLabel>
              {presets.map((preset) => (
                <DropdownMenuItem key={preset.label} onClick={() => onSend({ scheduledAt: preset.at })}>
                  <Clock className="text-muted-foreground" />
                  {preset.label}
                  <span className="ml-auto text-xs text-muted-foreground">
                    {new Intl.DateTimeFormat('en-US', { timeZone, weekday: 'short', month: 'short', day: 'numeric' }).format(preset.at)}
                  </span>
                </DropdownMenuItem>
              ))}
              <DropdownMenuItem onClick={() => setPicking(true)}>
                <Clock className="text-muted-foreground" />
                Pick date and time…
              </DropdownMenuItem>
            </DropdownMenuGroup>
            {inThread ? (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={() => onSend({ archive: true })}>
                  <Send className="text-muted-foreground" />
                  Send and archive
                  <DropdownMenuShortcut>
                    <ShortcutKeys keys="Mod+Shift+Enter" />
                  </DropdownMenuShortcut>
                </DropdownMenuItem>
              </>
            ) : null}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
      <Popover open={picking} onOpenChange={setPicking}>
        <PopoverContent anchor={group} side="top" align="start" className="w-auto p-0">
          {picking ? (
            <PickDateTime
              timeZone={timeZone}
              onCancel={() => setPicking(false)}
              onPick={(at) => {
                setPicking(false)
                onSend({ scheduledAt: at })
              }}
            />
          ) : null}
        </PopoverContent>
      </Popover>
    </div>
  )
}

const localDate = (day: CalendarDay) => new Date(day.year, day.month - 1, day.day)
const dayOf = (date: Date): CalendarDay => ({ year: date.getFullYear(), month: date.getMonth() + 1, day: date.getDate() })
const pad = (value: number) => String(value).padStart(2, '0')

/** A day and a time in the user's time zone; tomorrow at 8:00 to start with. */
function PickDateTime({ timeZone, onPick, onCancel }: { timeZone: string; onPick: (at: Date) => void; onCancel: () => void }) {
  const [now] = useState(() => new Date())
  const today = dayIn(now, timeZone)
  const [day, setDay] = useState<CalendarDay>(() => dayIn(new Date(now.getTime() + 86_400_000), timeZone))
  const [month, setMonth] = useState(() => localDate(day))
  const [time, setTime] = useState(`${pad(SEND_LATER_HOUR)}:00`)
  const at = zonedDateTime(day, time, timeZone)
  // checked against the moment the picker opened; a time that passed since is refused by the server, which says so
  const problem = !at ? 'Enter a time.' : at.getTime() <= now.getTime() ? 'Pick a time in the future.' : null

  return (
    <form
      className="flex flex-col gap-3 p-3"
      onSubmit={(event) => {
        event.preventDefault()
        if (at && !problem) onPick(at)
      }}
    >
      <Calendar
        mode="single"
        required
        selected={localDate(day)}
        month={month}
        onMonthChange={setMonth}
        disabled={{ before: localDate(today) }}
        onSelect={(date) => date && setDay(dayOf(date))}
        className="p-0"
      />
      <label className="flex items-center gap-2 text-[0.8125rem]">
        <Clock className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        <span className="sr-only">Time</span>
        <Input type="time" required value={time} onChange={(event) => setTime(event.target.value)} className="h-8 flex-1" />
      </label>
      <p className={cn('max-w-64 text-xs', problem ? 'text-destructive' : 'text-muted-foreground')} role={problem ? 'alert' : undefined}>
        {problem ?? `Sends ${sendTimeLabel(at!, now, timeZone, true)} (${timeZone.replace(/_/g, ' ')}).`}
      </p>
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" size="sm" onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" size="sm" disabled={problem !== null}>
          Schedule
        </Button>
      </div>
    </form>
  )
}
