import { useState } from 'react'
import { Xmark } from 'reicon-react'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { presenceOf, usePresence } from '@/features/realtime/presence'
import type { User } from '@/features/workspaces/models'
import { matchPeople } from './channelLib'

interface MemberPickerProps {
  /** The people on offer, in the order to show them (see `pickablePeople`). */
  people: User[]
  selectedIds: string[]
  onChange: (selectedIds: string[]) => void
  /** Accessible name of the search field. */
  label: string
  placeholder?: string
  autoFocus?: boolean
}

/**
 * Search field, chips for the chosen people and a list to choose from. Enter or a click toggles a person; Backspace in
 * an empty field removes the last chip. Shared by the new channel and new message dialogs and by "Add people".
 */
export function MemberPicker({ people, selectedIds, onChange, label, placeholder = 'Search people…', autoFocus = false }: MemberPickerProps) {
  const [search, setSearch] = useState('')
  const presence = usePresence()
  const selected = selectedIds.flatMap((id) => people.find((person) => person.id === id) ?? [])
  const results = matchPeople(people, search)

  const toggle = (id: string) => {
    onChange(selectedIds.includes(id) ? selectedIds.filter((selectedId) => selectedId !== id) : [...selectedIds, id])
  }

  return (
    <Command data-slot="member-picker" shouldFilter={false} label={label} className="h-auto gap-1 bg-transparent p-0">
      {selected.length > 0 ? (
        <ul aria-label="Chosen people" className="flex flex-wrap gap-1 px-1 pb-1">
          {selected.map((person) => (
            <li key={person.id}>
              <Badge variant="secondary" className="h-6 max-w-48 gap-1.5 pr-0 pl-1">
                <UserAvatar user={person} size={16} />
                <span className="truncate">{person.name}</span>
                <Button type="button" variant="ghost" size="icon-xs" className="rounded-full" aria-label={`Remove ${person.name}`} onClick={() => toggle(person.id)}>
                  <Xmark />
                </Button>
              </Badge>
            </li>
          ))}
        </ul>
      ) : null}
      <CommandInput
        autoFocus={autoFocus}
        placeholder={placeholder}
        value={search}
        onValueChange={setSearch}
        onKeyDown={(event) => {
          if (event.key === 'Backspace' && search === '' && selectedIds.length > 0) onChange(selectedIds.slice(0, -1))
        }}
      />
      <CommandList className="max-h-52">
        <CommandEmpty className="text-muted-foreground">{people.length === 0 ? 'There is nobody to add' : 'No people match'}</CommandEmpty>
        {results.map((person) => {
          const chosen = selectedIds.includes(person.id)
          return (
            <CommandItem
              key={person.id}
              value={person.id}
              data-checked={chosen}
              onSelect={() => {
                toggle(person.id)
                setSearch('')
              }}
            >
              <UserAvatar user={person} size={24} status={presenceOf(presence, person.id).status} />
              <span className="truncate">{person.name}</span>
              <span className="truncate text-xs text-muted-foreground">@{person.handle}</span>
              {chosen ? <span className="sr-only">Chosen</span> : null}
            </CommandItem>
          )
        })}
      </CommandList>
    </Command>
  )
}
