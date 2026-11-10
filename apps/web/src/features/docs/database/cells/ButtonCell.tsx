import { useState } from 'react'
import { cn } from 'cn'
import { buttonLabel } from '@orbit/database/buttons'
import type { Property, Row } from '@orbit/database/types'
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { usePressButton } from '../buttonApi'
import { useDatabaseEnv } from '../context'

type ButtonProperty = Extract<Property, { type: 'button' }>

/** A button property's cell (spec P3 17.4): a press asks the confirmation when set, then shows a spinner until the server answers. */
export function ButtonCell({ property, row, className }: { property: ButtonProperty; row: Row; className?: string }) {
  const { workspaceId, databaseId, canEditValues } = useDatabaseEnv()
  const { press, pending } = usePressButton(workspaceId, databaseId)
  const [confirming, setConfirming] = useState(false)
  const label = buttonLabel(property)
  const opensUrl = property.config.actions.some((action) => action.type === 'open_url')
  const run = () => press({ rowId: row.id, propertyId: property.id }, opensUrl)
  return (
    <>
      <Button
        type="button"
        variant="outline"
        size="xs"
        data-button-press=""
        className={cn('max-w-full', className)}
        disabled={!canEditValues || pending}
        aria-busy={pending || undefined}
        tabIndex={-1}
        onClick={(event) => {
          event.stopPropagation()
          if (property.config.confirm) setConfirming(true)
          else run()
        }}
        onDoubleClick={(event) => event.stopPropagation()}
      >
        {pending ? <Spinner className="size-3" /> : null}
        <span className="truncate">{label}</span>
      </Button>
      {/* The dialog renders in a portal, but its React events still bubble to the cell or card (a click would open the row). */}
      <span className="contents" onClick={(event) => event.stopPropagation()} onKeyDown={(event) => event.stopPropagation()}>
        <AlertDialog open={confirming} onOpenChange={setConfirming}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{label}</AlertDialogTitle>
              <AlertDialogDescription>{property.config.confirm}</AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <Button type="button" variant="outline" onClick={() => setConfirming(false)}>
                Cancel
              </Button>
              <Button
                type="button"
                onClick={() => {
                  setConfirming(false)
                  run()
                }}
              >
                Continue
              </Button>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </span>
    </>
  )
}
