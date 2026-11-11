import { useState } from 'react'
import { DropdownMenuGroup, DropdownMenuLabel } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { options } from '@orbit/database/values'
import type { Property, SelectOption } from '@orbit/database/types'
import { OptionMenu, withNewOption } from './cells/CellEditor'
import { Pill } from './cells/Pill'

/** The options of a select, multi-select or status in the "Edit property" menu: add, rename, recolor, delete. */
export function PropertyOptions({ property, onOptions }: { property: Property; onOptions: (options: SelectOption[]) => Promise<void> }) {
  const [name, setName] = useState('')
  const list = options(property)
  const add = () => {
    const trimmed = name.trim()
    if (!trimmed || list.some((option) => option.name.toLowerCase() === trimmed.toLowerCase())) return
    setName('')
    void onOptions(withNewOption(property, list, trimmed).next).catch(() => undefined)
  }

  return (
    <DropdownMenuGroup>
      <DropdownMenuLabel>Options</DropdownMenuLabel>
      {/* The menu must not take the keys typed in the name fields. */}
      <div className="flex flex-col gap-0.5 px-1 pb-1" onKeyDown={(event) => event.stopPropagation()}>
        {list.map((option) => (
          <div key={option.id} className="group/option flex h-8 items-center gap-2 rounded-md px-1.5 hover:bg-muted/60">
            <Pill color={option.color} dot={property.type === 'status'} className="min-w-0">{option.name}</Pill>
            <span className="flex-1" />
            <OptionMenu option={option} list={list} onOptions={onOptions} />
          </div>
        ))}
        <Input
          aria-label="New option"
          placeholder="Add an option…"
          value={name}
          maxLength={200}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') add()
          }}
        />
      </div>
    </DropdownMenuGroup>
  )
}
