// One recipient field (To, Cc or Bcc): chips, autocomplete from the user's contacts (members first, §9.1 U12),
// pasting many addresses at once, invalid addresses as red chips. Keyboard: Enter, Tab, comma or semicolon add;
// ↑/↓ move in the list. ← at the start of the field (or Backspace on an empty one) moves to the chips: ←/→ move
// between them, Enter edits one, Backspace or Delete removes it.
import { useEffect, useId, useImperativeHandle, useLayoutEffect, useRef, useState, type ClipboardEvent, type KeyboardEvent, type ReactNode, type Ref, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import type { MailContact } from '@orbit/api/generated/types.gen'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Tip } from '@/components/common/Tip'
import { useDebouncedValue } from '@/lib/useDebouncedValue'
import { useContacts } from '../api/queries'
import { commitTyped, isValidAddress, splitRecipients, suppressionText, type SuppressedRecipient } from './composeLib'

const sameAddress = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase()

/** Members first (§9.1 U12), then the server's order (the people the user writes to most). */
const membersFirst = (contacts: readonly MailContact[]) =>
  [...contacts].sort((a, b) => Number(b.kind === 'member') - Number(a.kind === 'member'))

export interface RecipientFieldHandle {
  /** Adds the text still typed in the field (as on send) and returns the list it makes. */
  commit: () => string[]
  focus: () => void
}

export interface RecipientFieldProps {
  label: 'To' | 'Cc' | 'Bcc'
  addresses: readonly string[]
  onChange: (addresses: string[]) => void
  /** Names known for addresses (the thread's people, picked contacts), shown on the chips. */
  names: ReadonlyMap<string, string>
  /** Recipients on the suppression list, by lower-case address: amber chips that say why. */
  suppressed?: ReadonlyMap<string, SuppressedRecipient>
  onName: (address: string, name: string) => void
  /** After the field (the Cc and Bcc toggles of the To row). */
  trailing?: ReactNode
  autoFocus?: boolean
  ref?: Ref<RecipientFieldHandle>
}

export function RecipientField({ label, addresses, onChange, names, suppressed, onName, trailing, autoFocus, ref }: RecipientFieldProps) {
  const input = useRef<HTMLInputElement>(null)
  const box = useRef<HTMLDivElement>(null)
  const chips = useRef<(HTMLButtonElement | null)[]>([])
  const listId = useId()
  const [text, setText] = useState('')
  const [focused, setFocused] = useState(false)
  const [active, setActive] = useState(0)
  const [dismissed, setDismissed] = useState(false)
  const query = useDebouncedValue(text.trim(), 120)
  const contacts = useContacts(focused && !text.includes(',') ? query : '')
  const suggestions = text.trim() === '' ? [] : membersFirst(contacts.data ?? []).filter((contact) => !addresses.some((address) => sameAddress(address, contact.address))).slice(0, 8)
  const open = focused && !dismissed && suggestions.length > 0

  useEffect(() => {
    if (autoFocus) input.current?.focus()
  }, [autoFocus])

  /** Sets the list once, when it changed. */
  const update = (next: string[]) => {
    if (next.length !== addresses.length || next.some((address, index) => address !== addresses[index])) onChange(next)
    return next
  }
  const commitText = () => {
    const next = commitTyped(addresses, text)
    setText('')
    setActive(0)
    return update(next)
  }

  useImperativeHandle(ref, () => ({ commit: commitText, focus: () => input.current?.focus() }))

  const pick = (contact: MailContact) => {
    if (contact.name?.trim()) onName(contact.address, contact.name.trim())
    update(commitTyped(addresses, contact.address))
    setText('')
    setActive(0)
    input.current?.focus()
  }
  /** Focuses the chip at `index` as rendered now, or the field when there is none. */
  const focusChip = (index: number) => {
    const chip = index >= 0 ? chips.current[index] : null
    if (chip?.isConnected) chip.focus()
    else input.current?.focus()
  }
  /** Removes a chip; from the keyboard, focus moves to the chip before it (Backspace) or after it (Delete). */
  const remove = (index: number, move?: 'back' | 'forward') => {
    const next = update(addresses.filter((_, at) => at !== index))
    if (!move) return
    // a neighbour in the new list: Backspace on the first chip goes to the new first one
    const target = next.length === 0 ? -1 : move === 'back' ? Math.max(0, index - 1) : Math.min(index, next.length - 1)
    // the list renders again first
    requestAnimationFrame(() => focusChip(target))
  }
  /** A chip goes back into the field to be fixed; text typed there is added first. One change of the list. */
  const edit = (index: number) => {
    const address = addresses[index]
    if (address === undefined) return
    update(commitTyped(addresses.filter((_, at) => at !== index), text))
    setText(address)
    input.current?.focus()
  }

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const key = event.key
    if (open && (key === 'ArrowDown' || key === 'ArrowUp')) {
      event.preventDefault()
      setActive((index) => (index + (key === 'ArrowDown' ? 1 : -1) + suggestions.length) % suggestions.length)
      return
    }
    if (key === 'Enter' || key === 'Tab') {
      if (event.metaKey || event.ctrlKey || event.nativeEvent.isComposing) return
      const suggestion = open ? suggestions[active] : undefined
      if (suggestion) {
        event.preventDefault()
        pick(suggestion)
      } else if (text.trim()) {
        // Tab adds the address and moves on; Enter adds it and stays
        if (key === 'Enter') event.preventDefault()
        commitText()
      } else if (key === 'Enter') {
        event.preventDefault()
      }
      return
    }
    if ((key === ',' || key === ';') && text.trim()) {
      event.preventDefault()
      commitText()
      return
    }
    if (key === 'Escape' && open) {
      event.preventDefault()
      event.stopPropagation()
      setDismissed(true)
      return
    }
    const atStart = event.currentTarget.selectionStart === 0 && event.currentTarget.selectionEnd === 0
    if (addresses.length > 0 && ((key === 'Backspace' && text === '') || (key === 'ArrowLeft' && atStart && !event.shiftKey))) {
      event.preventDefault()
      focusChip(addresses.length - 1)
    }
  }

  const onChipKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const key = event.key
    if (key === 'ArrowLeft') focusChip(Math.max(0, index - 1))
    else if (key === 'ArrowRight') focusChip(index + 1)
    else if (key === 'Backspace') remove(index, 'back')
    else if (key === 'Delete') remove(index, 'forward')
    else if (key === 'Enter' || key === 'F2') edit(index)
    else return
    event.preventDefault()
  }

  const onPaste = (event: ClipboardEvent<HTMLInputElement>) => {
    const pasted = event.clipboardData.getData('text')
    const entries = splitRecipients(pasted)
    // one plain word: let it be typed
    if (entries.length <= 1 && !/[,;\n]/.test(pasted)) return
    // the pasted addresses become chips; what was typed stays in the field
    event.preventDefault()
    update(entries.reduce<string[]>((list, entry) => commitTyped(list, entry), [...addresses]))
  }

  return (
    <div
      ref={box}
      className="flex min-h-9 items-start gap-2 border-b border-border/60 px-3 py-1"
      onMouseDown={(event) => {
        // a press on the empty part of the row focuses the field
        if (event.target === event.currentTarget) {
          event.preventDefault()
          input.current?.focus()
        }
      }}
    >
      <label htmlFor={`${listId}-input`} className="mt-[7px] w-8 shrink-0 text-[0.8125rem] text-muted-foreground">
        {label}
      </label>
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1 py-0.5">
        {addresses.map((address, index) => (
          <RecipientChip
            key={`${address}:${index}`}
            chipRef={(element) => {
              chips.current[index] = element
            }}
            address={address}
            name={names.get(address.toLowerCase())}
            suppressed={suppressed?.get(address.trim().toLowerCase())}
            onKeyDown={(event) => onChipKeyDown(event, index)}
            onEdit={() => edit(index)}
            onRemove={() => remove(index)}
          />
        ))}
        <input
          ref={input}
          id={`${listId}-input`}
          value={text}
          onChange={(event) => {
            setText(event.target.value)
            setActive(0)
            setDismissed(false)
          }}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
          onFocus={() => setFocused(true)}
          onBlur={() => {
            setFocused(false)
            if (text.trim()) commitText()
          }}
          role="combobox"
          aria-expanded={open}
          aria-controls={open ? listId : undefined}
          aria-activedescendant={open ? `${listId}-${active}` : undefined}
          aria-autocomplete="list"
          autoComplete="off"
          spellCheck={false}
          className="h-7 min-w-32 flex-1 bg-transparent text-[0.8125rem] text-foreground outline-none placeholder:text-muted-foreground/60"
        />
      </div>
      {trailing}
      {open ? <SuggestionList anchor={box} id={listId} suggestions={suggestions} active={active} onHover={setActive} onPick={pick} /> : null}
    </div>
  )
}

function RecipientChip({
  chipRef,
  address,
  name,
  suppressed,
  onKeyDown,
  onEdit,
  onRemove,
}: {
  chipRef: (element: HTMLButtonElement | null) => void
  address: string
  name: string | undefined
  suppressed: SuppressedRecipient | undefined
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => void
  onEdit: () => void
  onRemove: () => void
}) {
  const valid = isValidAddress(address)
  return (
    <Tip
      label={
        !valid
          ? 'Not a valid email address. Press Enter or click to fix it.'
          : suppressed
            ? `${name ? `${address}. ` : ''}${suppressionText(suppressed)}. Orbit will skip it.`
            : name
              ? address
              : ''
      }
    >
      <span
        data-slot="recipient-chip"
        data-invalid={!valid || undefined}
        className={cn(
          'group/chip inline-flex h-6 max-w-full items-center gap-0.5 rounded-md border pr-0.5 pl-2 text-[0.8125rem] transition-colors duration-100',
          'has-[button:first-child:focus-visible]:border-ring has-[button:first-child:focus-visible]:ring-2 has-[button:first-child:focus-visible]:ring-ring/40',
          !valid
            ? 'border-destructive/30 bg-destructive/10 text-destructive'
            : suppressed
              ? 'border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300'
              : 'border-transparent bg-muted text-foreground',
        )}
      >
        <button
          ref={chipRef}
          type="button"
          tabIndex={-1}
          aria-invalid={!valid || undefined}
          aria-keyshortcuts="Enter Backspace Delete"
          className="min-w-0 cursor-text truncate outline-none"
          onMouseDown={(event) => event.preventDefault()}
          onClick={onEdit}
          onKeyDown={onKeyDown}
        >
          {name ?? address}
          {!valid ? <span className="sr-only"> (invalid address)</span> : suppressed ? <span className="sr-only"> (suppressed: {suppressionText(suppressed)})</span> : null}
        </button>
        <button
          type="button"
          tabIndex={-1}
          aria-label={`Remove ${address}`}
          className="flex size-5 shrink-0 items-center justify-center rounded-[5px] text-muted-foreground opacity-60 transition-opacity duration-100 hover:bg-foreground/8 hover:text-foreground hover:opacity-100"
          onMouseDown={(event) => event.preventDefault()}
          onClick={onRemove}
        >
          <X className="size-3" />
        </button>
      </span>
    </Tip>
  )
}


/** The autocomplete list, in a portal under the field (a floating composer clips anything inside it). */
function SuggestionList({
  anchor,
  id,
  suggestions,
  active,
  onHover,
  onPick,
}: {
  anchor: RefObject<HTMLDivElement | null>
  id: string
  suggestions: readonly MailContact[]
  active: number
  onHover: (index: number) => void
  onPick: (contact: MailContact) => void
}) {
  const [place, setPlace] = useState<{ left: number; top: number; width: number } | null>(null)
  useLayoutEffect(() => {
    const measure = () => {
      const rect = anchor.current?.getBoundingClientRect()
      if (rect) setPlace({ left: rect.left + 44, top: rect.bottom + 4, width: Math.min(360, rect.width - 52) })
    }
    measure()
    window.addEventListener('resize', measure)
    window.addEventListener('scroll', measure, true)
    return () => {
      window.removeEventListener('resize', measure)
      window.removeEventListener('scroll', measure, true)
    }
  }, [anchor])
  if (!place) return null
  return createPortal(
    <div
      id={id}
      role="listbox"
      style={{ left: place.left, top: place.top, width: Math.max(240, place.width) }}
      className="fixed z-[90] max-h-72 overflow-y-auto overscroll-contain rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg"
      // keep the focus in the field
      onMouseDown={(event) => event.preventDefault()}
    >
      {suggestions.map((contact, index) => {
        const name = contact.name?.trim() || ''
        return (
          <div
            key={contact.address}
            id={`${id}-${index}`}
            role="option"
            aria-selected={index === active}
            data-active={index === active || undefined}
            className="flex cursor-default items-center gap-2.5 rounded-md px-2 py-1.5 data-active:bg-accent data-active:text-accent-foreground"
            onMouseMove={() => index !== active && onHover(index)}
            onClick={() => onPick(contact)}
          >
            <UserAvatar user={null} name={name || contact.address} size={24} />
            <div className="flex min-w-0 flex-col">
              <span className="truncate text-[0.8125rem] font-medium">{name || contact.address}</span>
              {name ? <span className="truncate text-xs text-muted-foreground">{contact.address}</span> : null}
            </div>
            {contact.kind === 'member' ? <span className="ml-auto shrink-0 text-[0.6875rem] text-muted-foreground/80">Member</span> : null}
          </div>
        )
      })}
    </div>,
    document.body,
  )
}
