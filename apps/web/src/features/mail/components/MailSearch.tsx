import { useId, useState, type Ref } from 'react'
import { SearchNormal } from 'reicon-react'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { searchSuggestions } from '@/features/mail/mailLib'

/**
 * The mail search box: words and the operators of spec §8.3 (`from:`, `to:`, `subject:`, `label:`, `in:`,
 * `is:unread`, `is:starred`, `has:attachment`, `before:`, `after:`), with completions for the word being typed.
 * Enter searches; ↑/↓ pick a completion, Enter or Tab takes it, Escape closes the list.
 */
export function MailSearch({
  defaultQuery,
  labels,
  people,
  inputRef,
  onSearch,
}: {
  defaultQuery: string
  /** Label names, for `label:`. */
  labels: readonly string[]
  /** Known addresses, for `from:` and `to:`. */
  people: readonly string[]
  inputRef?: Ref<HTMLInputElement>
  onSearch: (query: string) => void
}) {
  const listId = useId()
  const [value, setValue] = useState(defaultQuery)
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(-1)
  const suggestions = open ? searchSuggestions(value, { labels, people }) : []
  const shown = suggestions.length > 0

  const take = (query: string) => {
    setValue(query)
    setActive(-1)
  }

  return (
    <div className="relative min-w-0 flex-1">
      <InputGroup className="h-7">
        <InputGroupAddon>
          <SearchNormal className="size-3.5" />
        </InputGroupAddon>
        <InputGroupInput
          ref={inputRef}
          type="search"
          role="combobox"
          aria-label="Search mail"
          aria-autocomplete="list"
          aria-expanded={shown}
          aria-controls={shown ? listId : undefined}
          aria-activedescendant={shown && active >= 0 ? `${listId}-${active}` : undefined}
          placeholder="Search mail"
          className="text-[13px] md:text-[13px]"
          value={value}
          onChange={(event) => {
            setValue(event.target.value)
            setOpen(true)
            setActive(-1)
          }}
          onFocus={() => setOpen(true)}
          onBlur={() => setOpen(false)}
          onKeyDown={(event) => {
            if (event.key === 'ArrowDown' && shown) {
              event.preventDefault()
              setActive((index) => (index + 1) % suggestions.length)
            } else if (event.key === 'ArrowUp' && shown) {
              event.preventDefault()
              setActive((index) => (index <= 0 ? suggestions.length - 1 : index - 1))
            } else if (event.key === 'Tab' && shown && !event.shiftKey) {
              event.preventDefault()
              take(suggestions[Math.max(active, 0)].query)
            } else if (event.key === 'Enter') {
              event.preventDefault()
              if (shown && active >= 0) take(suggestions[active].query)
              else {
                setOpen(false)
                onSearch(value.trim())
              }
            } else if (event.key === 'Escape') {
              if (shown) {
                event.preventDefault()
                setOpen(false)
              } else event.currentTarget.blur()
            }
          }}
        />
      </InputGroup>
      {shown ? (
        <div
          id={listId}
          role="listbox"
          aria-label="Search suggestions"
          className="absolute inset-x-0 top-full z-50 mt-1 flex flex-col rounded-lg bg-popover p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10"
        >
          {suggestions.map((suggestion, index) => (
            <div
              key={suggestion.label}
              id={`${listId}-${index}`}
              role="option"
              aria-selected={index === active}
              data-active={index === active || undefined}
              className="cursor-default truncate rounded-md px-2 py-1 font-mono text-xs data-active:bg-accent data-active:text-accent-foreground hover:bg-accent hover:text-accent-foreground"
              // the input keeps the focus
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => take(suggestion.query)}
            >
              {suggestion.label}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  )
}
