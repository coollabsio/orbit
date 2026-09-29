import { Dialog as DialogPrimitive } from '@base-ui/react/dialog'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from 'cn'

/** A modal panel that slides in from a screen edge (mobile nav drawer, member list, thread pane).
    Base UI gives it Escape, focus trap, scroll lock, backdrop dismiss and keeps it mounted through the exit. */
function SideSheet(props: DialogPrimitive.Root.Props) {
  return <DialogPrimitive.Root data-slot="side-sheet" {...props} />
}

// At rest there is no transform, so fixed overlays inside the sheet are not trapped in a containing block.
const sideSheetContentVariants = cva(
  'fixed top-0 bottom-0 z-50 flex flex-col outline-none transition-transform duration-240 ease-drawer data-ending-style:duration-150 motion-reduce:transition-none',
  {
    variants: {
      side: {
        left: 'left-0 data-starting-style:-translate-x-full data-ending-style:-translate-x-full',
        right: 'right-0 data-starting-style:translate-x-full data-ending-style:translate-x-full',
      },
    },
    defaultVariants: { side: 'left' },
  },
)

function SideSheetContent({
  className,
  side,
  ...props
}: DialogPrimitive.Popup.Props & VariantProps<typeof sideSheetContentVariants>) {
  return (
    <DialogPrimitive.Portal data-slot="side-sheet-portal">
      <DialogPrimitive.Backdrop
        data-slot="side-sheet-overlay"
        className="fixed inset-0 z-40 bg-black/50 transition-opacity duration-200 ease-out data-ending-style:duration-150 data-starting-style:opacity-0 data-ending-style:opacity-0 motion-reduce:transition-none"
      />
      <DialogPrimitive.Popup
        data-slot="side-sheet-content"
        data-side={side ?? 'left'}
        className={cn(sideSheetContentVariants({ side }), className)}
        {...props}
      />
    </DialogPrimitive.Portal>
  )
}

export { SideSheet, SideSheetContent }
