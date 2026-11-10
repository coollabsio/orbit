import { Warning } from 'reicon-react'
import { formulaBase, formulaMessageParts, plainFormulaMessage, type FormulaProperty } from '@orbit/database/formula/values'
import type { Row } from '@orbit/database/types'
import { Tip } from '@/components/common/Tip'
import { CellView } from './CellView'

/** A formula cell: the server's result in its type's style, or a muted "Error" whose message shows on hover (spec P3 15.7). */
export function FormulaCell({ property, row, width, wrap }: { property: FormulaProperty; row: Row; width: number; wrap: boolean }) {
  const error = row.formulaErrors?.[property.id]
  if (error === undefined) return <CellView property={formulaBase(property)} row={row} width={width} wrap={wrap} />
  return (
    <Tip label={<FormulaMessage message={error} />}>
      <span className="flex min-w-0 items-center gap-1 text-muted-foreground" aria-label={`Error: ${plainFormulaMessage(error)}`}>
        <Warning className="size-3.5 shrink-0" />
        <span className="truncate">Error</span>
      </span>
    </Tip>
  )
}

/** A server formula message with its backtick spans shown as code. */
export function FormulaMessage({ message }: { message: string }) {
  return formulaMessageParts(message).map((part, index) =>
    part.code ? (
      <code key={index} className="rounded-sm bg-foreground/[0.08] px-1 font-mono text-[0.9em]">
        {part.text}
      </code>
    ) : (
      part.text
    ),
  )
}
