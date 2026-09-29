import { SearchNormal as Search } from 'reicon-react'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'

/** Quick search: local to the page, never saved into a view (a saved text filter is `text contains`). */
export function TaskSearchBox({ value, onChange, label = 'Search tasks' }: { value: string; onChange: (value: string) => void; label?: string }) {
  return (
    // content-sized (InputGroup carries w-full, see lessons); on phones it drops to its own full-width row
    <InputGroup className="w-auto min-w-45 max-[899px]:order-10 max-[899px]:mt-1 max-[899px]:h-[34px] max-[899px]:min-w-0 max-[899px]:basis-full">
      <InputGroupAddon align="inline-start">
        <Search className="size-3.5" aria-hidden="true" />
      </InputGroupAddon>
      <InputGroupInput
        type="search"
        aria-label={label}
        placeholder={label}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </InputGroup>
  )
}
