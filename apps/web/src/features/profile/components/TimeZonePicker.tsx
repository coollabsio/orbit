import { useState } from 'react'
import { Check, ChevronDown } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { currentTimeZone, localTimeLabel, timeZones } from '../profileLib'

/** "America/New_York" as people read it. */
const zoneLabel = (zone: string) => zone.replaceAll('_', ' ')

/**
 * The time zone of the profile: a searchable list of the IANA zones this browser knows, the zone of this device in
 * one click, and a way to have none. `value` is the IANA name, `''` for none.
 */
export function TimeZonePicker({ id, value, onChange }: { id: string; value: string; onChange: (zone: string) => void }) {
  const [open, setOpen] = useState(false)
  // the list of the browser lacks `UTC`, and a saved zone must always be in it
  const [known] = useState(timeZones)
  const zones = [...new Set([...known, 'UTC', ...(value ? [value] : [])])]
  const here = currentTimeZone()
  const time = localTimeLabel(value)

  return (
    <div data-slot="time-zone-picker" className="flex flex-col gap-1.5">
      <Popover open={open} onOpenChange={setOpen} modal={false}>
        <PopoverTrigger render={<Button id={id} type="button" variant="outline" className="w-full justify-between font-normal" />}>
          <span className="truncate data-empty:text-muted-foreground" data-empty={value ? undefined : ''}>
            {value ? zoneLabel(value) : 'Not set'}
          </span>
          <ChevronDown className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        </PopoverTrigger>
        <PopoverContent align="start" className="w-(--anchor-width) min-w-64 gap-0 p-0">
          <Command label="Time zone">
            <CommandInput autoFocus aria-label="Search time zones" placeholder="Search time zones…" />
            <CommandList>
              <CommandEmpty className="py-4 text-xs text-muted-foreground">No time zone matches</CommandEmpty>
              <CommandGroup>
                {zones.map((zone) => (
                  <CommandItem
                    key={zone}
                    value={zone}
                    keywords={[zoneLabel(zone)]}
                    onSelect={() => {
                      onChange(zone)
                      setOpen(false)
                    }}
                  >
                    <span className="flex-1 truncate">{zoneLabel(zone)}</span>
                    {zone === value ? <Check className="size-3.5" aria-hidden="true" /> : null}
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
        {time ? <span>Now {time}</span> : null}
        {here && here !== value ? (
          <Button type="button" variant="link" size="xs" className="h-auto p-0" onClick={() => onChange(here)}>
            Use my current time zone
          </Button>
        ) : null}
        {value ? (
          <Button type="button" variant="link" size="xs" className="h-auto p-0 text-muted-foreground" onClick={() => onChange('')}>
            Clear
          </Button>
        ) : null}
      </div>
    </div>
  )
}
