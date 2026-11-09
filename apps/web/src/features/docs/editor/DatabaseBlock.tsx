import { useState, type DragEvent } from 'react'
import { ISOLATED_ATTR } from '@orbit/editor/isolation'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

type DatabaseBlockViewProps = { block: { props: { databaseId: string; linkId: string } } }

const ISOLATED = { [ISOLATED_ATTR]: '' }

/** `render` of the `database` block spec. */
export function DatabaseBlockView({ block }: DatabaseBlockViewProps) {
  const [text, setText] = useState('')
  const [items, setItems] = useState(['First', 'Second', 'Third'])
  const [dragged, setDragged] = useState<string | null>(null)

  const onDrop = (event: DragEvent, target: string) => {
    event.preventDefault()
    if (!dragged || dragged === target) return
    setItems((list) => {
      const rest = list.filter((item) => item !== dragged)
      rest.splice(rest.indexOf(target), 0, dragged)
      return rest
    })
    setDragged(null)
  }

  return (
    <div
      {...ISOLATED}
      contentEditable={false}
      data-slot="database-block"
      data-database-id={block.props.databaseId}
      className="flex w-full min-w-0 flex-col gap-2 rounded-md border border-border p-3 text-sm"
    >
      <Input aria-label="Prototype input" value={text} onChange={(event) => setText(event.target.value)} />
      <Popover modal={false}>
        <PopoverTrigger render={<button type="button" className="w-fit rounded-md border px-2 py-1">Open popover</button>} />
        <PopoverContent>
          <Input aria-label="Popover input" />
        </PopoverContent>
      </Popover>
      <ul className="flex flex-col gap-1">
        {items.map((item) => (
          <li
            key={item}
            draggable
            data-dragging={dragged === item || undefined}
            onDragStart={(event) => {
              event.dataTransfer.setData('text/plain', item)
              setDragged(item)
            }}
            onDragEnd={() => setDragged(null)}
            onDragOver={(event) => event.preventDefault()}
            onDrop={(event) => onDrop(event, item)}
            className="cursor-grab rounded-sm bg-secondary/40 px-2 py-1 data-dragging:opacity-50"
          >
            {item}
          </li>
        ))}
      </ul>
    </div>
  )
}
