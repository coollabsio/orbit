import { useState } from 'react'
import { cn } from 'cn'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { CALCULATION_LABELS, calculate, calculationsFor } from '@orbit/database/calculate'
import type { DatabaseCalculation, Property, Row, ViewConfig } from '@orbit/database/types'
import { useDatabaseEnv } from './context'

interface CalculationFooterProps {
  columns: { property: Property; width: number }[]
  /** Every row of the view after filter and search, not only the rows a block shows. */
  rows: readonly Row[]
  config: ViewConfig
  onConfig: (config: ViewConfig) => void
}

/** The table's footer (spec P2 4.5): one calculation per column, chosen from a menu; a change is a view change. */
export function CalculationFooter({ columns, rows, config, onConfig }: CalculationFooterProps) {
  const setCalculation = (propertyId: string, fn: DatabaseCalculation | null) => {
    const { [propertyId]: _replaced, ...rest } = config.calculations
    onConfig({ ...config, calculations: fn ? { ...rest, [propertyId]: fn } : rest })
  }
  return (
    <div role="rowgroup" className="sticky bottom-0 z-10 bg-background">
      <div role="row" aria-label="Calculations" className="group/footer flex h-[33px] text-sm text-muted-foreground">
        {columns.map(({ property, width }) => (
          <div key={property.id} role="gridcell" className="flex shrink-0" style={{ width }}>
            <CalculationCell property={property} rows={rows} fn={config.calculations[property.id] ?? null} onChange={(fn) => setCalculation(property.id, fn)} />
          </div>
        ))}
      </div>
    </div>
  )
}

function CalculationCell({ property, rows, fn, onChange }: { property: Property; rows: readonly Row[]; fn: DatabaseCalculation | null; onChange: (fn: DatabaseCalculation | null) => void }) {
  const { ctx } = useDatabaseEnv()
  const [open, setOpen] = useState(false)
  const result = fn ? calculate(rows, property, fn, ctx) : null
  return (
    <DropdownMenu open={open} onOpenChange={setOpen} modal={false}>
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            aria-label={fn ? `${property.name}: ${CALCULATION_LABELS[fn].name} ${result?.value}` : `Calculate ${property.name}`}
            className="flex min-w-0 flex-1 items-center justify-end gap-1.5 px-2 outline-none hover:bg-muted/60 focus-visible:bg-muted/60 aria-expanded:bg-muted/60"
          />
        }
      >
        {fn && result ? (
          <>
            <span className="shrink-0 text-xs">{CALCULATION_LABELS[fn].short}</span>
            <span className="truncate text-foreground tabular-nums">{result.value}</span>
          </>
        ) : (
          <span className={cn('truncate text-xs', !open && 'hover-fine:opacity-0 hover-fine:group-hover/footer:opacity-100')}>Calculate</span>
        )}
      </DropdownMenuTrigger>
      {open ? (
        <DropdownMenuContent align="end" className="w-52">
          <DropdownMenuRadioGroup value={fn ?? 'none'} onValueChange={(value: string) => onChange(value === 'none' ? null : (value as DatabaseCalculation))}>
            <DropdownMenuRadioItem value="none" closeOnClick>
              None
            </DropdownMenuRadioItem>
            <DropdownMenuSeparator />
            {calculationsFor(property.type).map((option) => (
              <DropdownMenuRadioItem key={option} value={option} closeOnClick>
                {CALCULATION_LABELS[option].name}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      ) : null}
    </DropdownMenu>
  )
}
