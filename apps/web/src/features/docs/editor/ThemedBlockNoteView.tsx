import type { ComponentProps } from 'react'
import type { BlockSchema, InlineContentSchema, StyleSchema } from '@blocknote/core'
import { BlockNoteView } from '@blocknote/shadcn'
import { cn } from 'cn'

/**
 * BlockNote's shadcn view with its theme variables mapped onto the app's shadcn tokens (src/index.css :root / .dark).
 * BlockNote copies `className` (with `bn-root`, the scheme class and `data-color-scheme`) onto its portal roots, so the
 * mapping also reaches menus and toolbars. BlockNote's stylesheet is unlayered, so the `!` is what beats its
 * `.bn-root[data-color-scheme="dark"]` values from the utilities layer.
 */
export function ThemedBlockNoteView<B extends BlockSchema, I extends InlineContentSchema, S extends StyleSchema>({
  className,
  ...props
}: ComponentProps<typeof BlockNoteView<B, I, S>>) {
  return (
    <BlockNoteView<B, I, S>
      className={cn(
        '[--bn-border-radius:calc(var(--radius)*0.8)]! [--bn-font-family:inherit]!',
        '[--bn-colors-editor-background:transparent]! [--bn-colors-editor-text:var(--foreground)]!',
        '[--bn-colors-menu-background:var(--popover)]! [--bn-colors-menu-text:var(--popover-foreground)]!',
        '[--bn-colors-tooltip-background:var(--popover)]! [--bn-colors-tooltip-text:var(--popover-foreground)]!',
        '[--bn-colors-hovered-background:var(--accent)]! [--bn-colors-hovered-text:var(--accent-foreground)]!',
        '[--bn-colors-selected-background:var(--primary)]! [--bn-colors-selected-text:var(--primary-foreground)]!',
        '[--bn-colors-disabled-background:var(--muted)]! [--bn-colors-disabled-text:var(--muted-foreground)]!',
        '[--bn-colors-border:var(--border)]! [--bn-colors-shadow:color-mix(in_oklab,var(--foreground)_12%,transparent)]!',
        '[--bn-colors-side-menu:var(--muted-foreground)]!',
        // BlockNote draws a 1px indent line on each nested block. It sits in the margin beside a table and inside a callout.
        '[&_.bn-block-group_.bn-block:not(:has(.bn-toggle-wrapper))_.bn-block-group_.bn-block-outer:not([data-prev-depth-changed])]:before:border-l-0!',
        // BlockNote's unlayered `.bn-shadcn svg { width: revert }` beats the shadcn `size-4` icon rule, so menu
        // chevrons fall back to their 24px size attribute. The same rule makes the open/close mark 1em (it grows
        // with a heading). Keep that mark at 16px. The empty-toggle "add" line was indented for an 18px mark;
        // 20px lines it up with the text after the smaller mark.
        // The block-handle grip is a dropdown trigger. Leave its icon to the handle rules below.
        "[&_[data-slot^=dropdown-menu-]:not(.bn-button)_svg:not([class*='size-'])]:size-4!",
        // Block handle, matched to a Notion screenshot (144 dpi, so 1 CSS px = 2 image px).
        // Plus glyph sits inside a 16px icon (the icon pads the stroke). Grip icon stays 18px, in an
        // 18×24 rounded button with no fill until hover. 3px between the buttons reads as a 6px gap
        // after the plus pad. 19px of menu padding is the gap from the grip to the text: it is inside
        // the menu box, so the menu stays that far from the block.
        '[&_.bn-side-menu]:gap-[3px]! [&_.bn-side-menu]:pr-[19px]!',
        '[&_.bn-side-menu_.bn-button]:h-6! [&_.bn-side-menu_.bn-button]:w-[18px]! [&_.bn-side-menu_.bn-button]:min-w-0! [&_.bn-side-menu_.bn-button]:rounded-[4px]! [&_.bn-side-menu_.bn-button]:p-0! [&_.bn-side-menu_.bn-button]:text-muted-foreground!',
        '[&_.bn-side-menu_.bn-button:has([data-test=dragHandleAdd])]:bg-transparent! [&_.bn-side-menu_.bn-button:has([data-test=dragHandleAdd])]:hover:bg-foreground/6!',
        '[&_.bn-side-menu_.bn-button:has([data-test=dragHandle])]:bg-transparent! [&_.bn-side-menu_.bn-button:has([data-test=dragHandle])]:aria-expanded:bg-transparent! [&_.bn-side-menu_.bn-button:has([data-test=dragHandle])]:hover:bg-foreground/6!',
        '[&_.bn-side-menu_.bn-button_svg]:size-[18px]! [&_.bn-side-menu_.bn-button:has([data-test=dragHandleAdd])_svg]:size-4!',
        '[&_.bn-toggle-button>svg]:size-4! [&_.bn-toggle-add-block-button]:ml-5!',
        // The open state turns the mark 90°. Animate that turn. Reduced motion: no turn.
        '[&_.bn-toggle-button]:transition-transform! [&_.bn-toggle-button]:duration-150! [&_.bn-toggle-button]:ease-[cubic-bezier(0.23,1,0.32,1)]! motion-reduce:[&_.bn-toggle-button]:transition-none!',
        className,
      )}
      {...props}
    />
  )
}
