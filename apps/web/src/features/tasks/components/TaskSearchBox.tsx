import { SearchNormal as Search } from 'reicon-react'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'

/** Content-sized wrapper (InputGroup carries w-full; see lessons); the one focus ring lives on it. */
const SEARCH_GROUP =
  'h-8 w-auto min-w-[180px] rounded-lg border border-input bg-muted transition-colors has-[[data-slot=input-group-control]:focus-visible]:border-ring has-[[data-slot=input-group-control]:focus-visible]:ring-1 has-[[data-slot=input-group-control]:focus-visible]:ring-ring dark:bg-muted max-[899px]:order-10 max-[899px]:mt-1 max-[899px]:h-[34px] max-[899px]:min-w-0 max-[899px]:basis-full'

/** Quick search: local to the page, never saved into a view (a saved text filter is `text contains`). */
export function TaskSearchBox({ value, onChange, label = 'Search tasks' }: { value: string; onChange: (value: string) => void; label?: string }) {
  return (
    <InputGroup className={SEARCH_GROUP}>
      <InputGroupAddon align="inline-start" className="pl-[9px]">
        <Search className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      </InputGroupAddon>
      <InputGroupInput
        type="search"
        aria-label={label}
        placeholder={label}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="h-auto border-0 text-sm text-foreground shadow-none outline-none placeholder:text-muted-foreground focus:outline-none focus-visible:ring-0 md:text-sm"
      />
    </InputGroup>
  )
}
