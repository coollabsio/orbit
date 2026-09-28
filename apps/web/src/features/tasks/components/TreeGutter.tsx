import { ChevronRight } from 'reicon-react'
import { cn } from 'cn'
import { CHEVRON } from '@/features/views/components/GroupIcon'
import { MAX_TREE_INDENT, TREE_INDENT_PX } from '@/features/views/taskTree'

const CHEVRON_SLOT_PX = 16

export interface TreeGutterProps {
  /** Nesting depth (0 = root). Indentation stops at `MAX_TREE_INDENT`; deeper rows keep that indent. */
  depth: number
  /** Shows the chevron; a leaf keeps the empty 16px slot so titles stay aligned. */
  hasChildren: boolean
  expanded: boolean
  onToggle: () => void
  /** Task identifier for the chevron's name: "Collapse sub-issues of ORB-12". */
  identifier: string
  /** Extra left offset in px before the first level (the detail section pads its rows). */
  inset?: number
  className?: string
}

/**
 * Tree indentation shared by the nested list and the task detail's Sub-issues section: 16px per level, a 1px
 * guide per level through the full gutter height (centred under that level's chevron), then a 16px chevron slot.
 * Stretch it to the row height (`self-stretch`, cancel the row's vertical padding) so guides join between rows.
 */
export function TreeGutter({ depth, hasChildren, expanded, onToggle, identifier, inset = 0, className }: TreeGutterProps) {
  const indent = Math.min(depth, MAX_TREE_INDENT)
  const pad = inset + indent * TREE_INDENT_PX
  // border-box width: the padding plus the 16px chevron slot, so a leaf needs no spacer element
  return (
    <span className={cn('relative flex shrink-0 items-center self-stretch', className)} style={{ paddingLeft: pad, width: pad + CHEVRON_SLOT_PX }}>
      {Array.from({ length: indent }, (_, level) => (
        <span key={level} data-slot="tree-guide" aria-hidden className="absolute inset-y-0 w-px bg-border/60" style={{ left: inset + level * TREE_INDENT_PX + 7 }} />
      ))}
      {hasChildren ? (
        <button
          type="button"
          aria-expanded={expanded}
          aria-label={`${expanded ? 'Collapse' : 'Expand'} sub-issues of ${identifier}`}
          // the ::after overlay grows the 16px chevron to a 24px hit area
          className="relative flex size-4 cursor-pointer items-center justify-center rounded-sm text-muted-foreground/70 outline-none transition-colors duration-150 after:absolute after:-inset-1 hover-fine:hover:bg-accent hover-fine:hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          onClick={(event) => {
            // the row behind it opens the task
            event.stopPropagation()
            onToggle()
          }}
        >
          <ChevronRight aria-hidden className={cn(CHEVRON, expanded && 'rotate-90')} />
        </button>
      ) : null}
    </span>
  )
}
