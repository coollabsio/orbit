import { Fragment } from 'react'
import { Emoji } from '@/components/common/Emoji'
import { Button } from '@/components/ui/button'
import { SideSheet, SideSheetContent } from '@/components/common/SideSheet'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator } from '@/components/ui/dropdown-menu'
import type { MenuAnchor, MessageAction } from './useMessageActions'

/**
 * The message menu, for both `…` and right click. One instance for the whole list, placed at an anchor: a menu in
 * every row would mount hundreds of popups.
 */
export function MessageMenu({
  open,
  onOpenChange,
  anchor,
  align,
  actions,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  anchor: MenuAnchor
  align: 'start' | 'end'
  actions: MessageAction[]
}) {
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange}>
      <DropdownMenuContent
        anchor={anchor}
        side="bottom"
        align={align}
        sideOffset={align === 'end' ? 4 : 0}
        // The list puts focus back on the message row itself; "Add reaction" and "Edit" move it on from there.
        finalFocus={false}
        className="w-auto min-w-48"
      >
        {actions.map((action) => (
          <Fragment key={action.key}>
            {action.destructive ? <DropdownMenuSeparator /> : null}
            <DropdownMenuItem variant={action.destructive ? 'destructive' : 'default'} onClick={action.run}>
              <action.icon />
              {action.label}
            </DropdownMenuItem>
          </Fragment>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/**
 * The long-press sheet on a phone: quick reactions first, then the same actions as the menu. It rises from the bottom
 * with the `SideSheet` timing; the side classes are replaced by bottom ones.
 */
export function MessageActionSheet({
  open,
  onOpenChange,
  quickEmojis,
  onReact,
  actions,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  quickEmojis: string[]
  /** Absent for a message that cannot take reactions (deleted). */
  onReact?: (emoji: string) => void
  actions: MessageAction[]
}) {
  return (
    <SideSheet open={open} onOpenChange={onOpenChange}>
      <SideSheetContent
        aria-label="Message actions"
        data-slot="message-action-sheet"
        // The list puts focus on the message row; "Edit" then moves it into the edit field.
        finalFocus={false}
        className="top-auto right-0 max-h-[80dvh] overflow-y-auto rounded-t-xl border-t border-border bg-popover p-2 pb-[max(0.5rem,env(safe-area-inset-bottom))] text-popover-foreground data-ending-style:translate-x-0 data-ending-style:translate-y-full data-starting-style:translate-x-0 data-starting-style:translate-y-full"
      >
        {onReact ? (
          <div className="flex items-center justify-around gap-2 border-b border-border px-2 pt-1 pb-3">
            {quickEmojis.map((emoji) => (
              <Button
                key={emoji}
                variant="secondary"
                size="icon-lg"
                className="size-11 rounded-full text-xl"
                aria-label={`React with ${emoji}`}
                onClick={() => {
                  onReact(emoji)
                  onOpenChange(false)
                }}
              >
                <Emoji value={emoji} />
              </Button>
            ))}
          </div>
        ) : null}
        <div className="flex flex-col pt-1">
          {actions.map((action) => (
            <Button
              key={action.key}
              variant="ghost"
              data-variant={action.destructive ? 'destructive' : undefined}
              className="h-11 w-full justify-start gap-3 px-3 text-sm font-normal data-[variant=destructive]:text-destructive"
              onClick={() => {
                onOpenChange(false)
                action.run()
              }}
            >
              <action.icon weight="Filled" className="size-5" />
              {action.label}
            </Button>
          ))}
        </div>
      </SideSheetContent>
    </SideSheet>
  )
}
