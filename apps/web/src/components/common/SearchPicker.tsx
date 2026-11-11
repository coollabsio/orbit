import { useState, type ReactElement, type ReactNode } from 'react'
import { cn } from 'cn'
import { CheckMark } from '@/components/common/CheckMark'
import { Tip } from '@/components/common/Tip'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

export interface SearchPickerOption {
  value: string
  label: string
  /** More text the search matches, such as an email or a project key. */
  keywords?: string[]
  icon?: ReactNode
  /** Muted text at the end of the row. */
  hint?: ReactNode
}

/** True when every word of `query` is in one of `texts`, case-insensitive. An empty query matches all. */
export function matchesSearch(query: string, texts: string[]): boolean {
  const haystack = texts.join('\n').toLowerCase()
  return query.toLowerCase().split(/\s+/).filter(Boolean).every((word) => haystack.includes(word))
}

/**
 * A searchable list in a popover, for long lists from data (people, projects, repositories). The search matches the
 * label and the keywords, never the value (an id). Arrow keys and Enter pick, Escape closes. With `multiple`, each row
 * has a checkbox and the popover stays open while you toggle, unless `closeOnSelect`.
 */
export function SearchPicker({
  trigger,
  label,
  placeholder,
  emptyText,
  options,
  selected,
  onSelect,
  multiple = false,
  closeOnSelect = !multiple,
  tip,
  className,
}: {
  trigger: ReactElement
  label: string
  placeholder: string
  emptyText: string
  options: SearchPickerOption[]
  selected: string[]
  onSelect: (value: string) => void
  multiple?: boolean
  closeOnSelect?: boolean
  /** A tooltip on the trigger. */
  tip?: string
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const shown = options.filter((option) => matchesSearch(query, [option.label, ...(option.keywords ?? [])]))
  return (
    <Popover open={open} onOpenChange={(next) => { setOpen(next); if (!next) setQuery('') }}>
      {tip ? <Tip label={tip}><PopoverTrigger render={trigger} /></Tip> : <PopoverTrigger render={trigger} />}
      <PopoverContent align="start" className={cn('w-60 gap-0 p-0', className)}>
        <Command label={label} shouldFilter={false}>
          <CommandInput autoFocus aria-label={label} placeholder={placeholder} value={query} onValueChange={setQuery} />
          <CommandList>
            <CommandEmpty className="py-4 text-xs text-muted-foreground">{emptyText}</CommandEmpty>
            {shown.length > 0 ? (
              <CommandGroup>
                {shown.map((option) => {
                  const checked = selected.includes(option.value)
                  return (
                    <CommandItem
                      key={option.value}
                      value={option.value}
                      aria-checked={checked}
                      // single choice: the item's own check mark at the end
                      data-checked={!multiple && checked}
                      onSelect={() => {
                        onSelect(option.value)
                        if (closeOnSelect) setOpen(false)
                      }}
                    >
                      {multiple ? <CheckMark checked={checked} /> : null}
                      {option.icon}
                      <span className="min-w-0 flex-1 truncate">{option.label}</span>
                      {option.hint ? <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{option.hint}</span> : null}
                    </CommandItem>
                  )
                })}
              </CommandGroup>
            ) : null}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}
