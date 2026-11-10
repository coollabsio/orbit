import { useRef, useState, type ReactElement } from 'react'
import { Add, Xmark } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { DatabaseSortDirection, DatabaseViewSort, Property, ViewConfig } from '@orbit/database/types'
import { PropertyPicker } from './FilterBar'
import { GripIcon } from './GripIcon'

const MAX_SORTS = 10
const DIRECTIONS = [
  { value: 'asc', label: 'Ascending' },
  { value: 'desc', label: 'Descending' },
]

/** The sorts of the view (spec 7.1): property, direction, drag to reorder, remove, "+ Add sort". */
export function SortPopover({ properties, config, onConfig, trigger }: {
  properties: Property[]
  config: ViewConfig
  onConfig: (config: ViewConfig) => void
  trigger: ReactElement
}) {
  const [adding, setAdding] = useState(false)
  const dragFrom = useRef<number | null>(null)
  const sorts = config.sorts
  const setSorts = (next: DatabaseViewSort[]) => onConfig({ ...config, sorts: next })
  const unsorted = properties.filter((property) => !sorts.some((sort) => sort.property_id === property.id))
  const propertyItems = (current: string) =>
    properties
      .filter((property) => property.id === current || unsorted.includes(property))
      .map((property) => ({ value: property.id, label: property.name }))

  return (
    <Popover>
      <PopoverTrigger render={trigger} />
      <PopoverContent align="end" className="w-auto min-w-72 gap-1 p-1.5">
        {sorts.length === 0 ? <p className="px-1.5 py-1 text-sm text-muted-foreground">No sorts. Rows keep their manual order.</p> : null}
        <ul aria-label="Sorts" className="flex flex-col gap-1">
          {sorts.map((sort, index) => (
            <li
              key={sort.property_id}
              className="flex items-center gap-1.5"
              onDragOver={(event) => {
                if (dragFrom.current !== null) event.preventDefault()
              }}
              onDrop={(event) => {
                event.preventDefault()
                const from = dragFrom.current
                dragFrom.current = null
                if (from === null || from === index) return
                const next = [...sorts]
                const [moved] = next.splice(from, 1)
                next.splice(index, 0, moved)
                setSorts(next)
              }}
            >
              <span
                draggable
                role="button"
                tabIndex={-1}
                aria-label="Drag to reorder"
                className="flex h-7 w-4 cursor-grab items-center justify-center text-muted-foreground"
                onDragStart={(event) => {
                  dragFrom.current = index
                  event.dataTransfer.effectAllowed = 'move'
                  event.dataTransfer.setData('text/plain', sort.property_id)
                }}
                onDragEnd={() => {
                  dragFrom.current = null
                }}
              >
                <GripIcon />
              </span>
              <Select
                items={propertyItems(sort.property_id)}
                value={sort.property_id}
                onValueChange={(value) => value && setSorts(sorts.map((other, at) => (at === index ? { ...other, property_id: value as string } : other)))}
              >
                <SelectTrigger size="sm" aria-label="Sort property" className="w-40">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {propertyItems(sort.property_id).map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Select
                items={DIRECTIONS}
                value={sort.direction}
                onValueChange={(value) => value && setSorts(sorts.map((other, at) => (at === index ? { ...other, direction: value as DatabaseSortDirection } : other)))}
              >
                <SelectTrigger size="sm" aria-label="Sort direction" className="w-32">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {DIRECTIONS.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                aria-label="Remove sort"
                className="text-muted-foreground"
                onClick={() => setSorts(sorts.filter((_, at) => at !== index))}
              >
                <Xmark />
              </Button>
            </li>
          ))}
        </ul>
        {sorts.length < MAX_SORTS && unsorted.length > 0 ? (
          <PropertyPicker
            properties={unsorted}
            label="Sort by"
            open={adding}
            onOpenChange={setAdding}
            onPick={(property) => setSorts([...sorts, { property_id: property.id, direction: 'asc' }])}
            trigger={
              <Button type="button" variant="ghost" size="sm" className="justify-start text-muted-foreground">
                <Add />
                Add sort
              </Button>
            }
          />
        ) : null}
        {sorts.length > 0 ? (
          <Button type="button" variant="ghost" size="sm" className="justify-start text-muted-foreground" onClick={() => setSorts([])}>
            Delete all sorts
          </Button>
        ) : null}
      </PopoverContent>
    </Popover>
  )
}
