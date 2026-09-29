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
        className,
      )}
      {...props}
    />
  )
}
