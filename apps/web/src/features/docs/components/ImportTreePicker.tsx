import { useMemo, useState, type ReactNode } from 'react'
import { ChevronRight, Search4 as Search } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { cn } from 'cn'
import {
  checkStates,
  nodeTitle,
  selectAll,
  selectedCount,
  toggleNode,
  visibleRows,
  type ImportNode,
  type ImportTree,
} from '@/features/docs/import/selection'

/** Rendering thousands of search hits at once is slow and useless; ask for a narrower query instead. */
const MAX_ROWS = 1000

interface Props<T extends ImportNode & { child_count: number }> {
  tree: ImportTree<T>
  selected: ReadonlySet<string>
  onSelectedChange: (selected: ReadonlySet<string>) => void
  /** The tree's accessible name ("Notion pages"). */
  label: string
  /** Shown when the scan found nothing to import. */
  empty: ReactNode
  icon: (node: T) => ReactNode
}

/** The filter, "N pages selected" and the tri-state tree of a Docs import's "Choose pages" step. */
export function ImportTreePicker<T extends ImportNode & { child_count: number }>({
  tree,
  selected,
  onSelectedChange,
  label,
  empty,
  icon,
}: Props<T>) {
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set())
  const [query, setQuery] = useState('')
  const states = useMemo(() => checkStates(tree, selected), [tree, selected])
  const rows = useMemo(() => visibleRows(tree, expanded, query), [tree, expanded, query])
  const count = selectedCount(tree, selected)
  const toggle = (id: string) => onSelectedChange(toggleNode(tree, selected, id))
  const toggleExpanded = (id: string) =>
    setExpanded((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  return (
    <>
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-2">
        <InputGroup className="w-64 rounded-lg border-border bg-foreground/[0.02] max-[899px]:w-full">
          <InputGroupAddon align="inline-start">
            <Search className="size-3.5 text-muted-foreground/70" />
          </InputGroupAddon>
          <InputGroupInput
            type="search"
            className="text-xs"
            placeholder="Filter pages"
            aria-label="Filter pages"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </InputGroup>
        <span className="flex-1" />
        <span className="text-xs text-muted-foreground tabular-nums" data-testid="selected-count">
          {count} {count === 1 ? 'page' : 'pages'} selected
        </span>
        <Button type="button" variant="ghost" size="sm" onClick={() => onSelectedChange(selectAll(tree))}>
          Select all
        </Button>
        <Button type="button" variant="ghost" size="sm" onClick={() => onSelectedChange(new Set())}>
          Clear
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2" role="tree" aria-label={label} aria-multiselectable="true">
        {tree.ids.length === 0 ? (
          <p className="p-4 text-sm text-muted-foreground">{empty}</p>
        ) : rows.length === 0 ? (
          <p className="p-4 text-sm text-muted-foreground">No pages match “{query.trim()}”.</p>
        ) : (
          rows.slice(0, MAX_ROWS).map((row) => {
            const state = states.get(row.id) ?? 'unchecked'
            const title = nodeTitle(row.node)
            return (
              <div
                key={row.id}
                role="treeitem"
                aria-level={row.depth + 1}
                aria-selected={state === 'checked'}
                aria-expanded={row.hasChildren ? row.expanded : undefined}
                className="flex h-8 min-w-0 items-center gap-1.5 rounded-md pr-2 text-[0.8125rem] transition-colors duration-150 ease-out hover:bg-sidebar-accent/50"
                style={{ paddingLeft: 4 + row.depth * 18 }}
              >
                {row.hasChildren && !query.trim() ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-xs"
                    className="size-5 shrink-0 text-muted-foreground/70 [&>svg]:transition-transform data-[expanded=true]:[&>svg]:rotate-90"
                    data-expanded={row.expanded}
                    aria-label={`${row.expanded ? 'Collapse' : 'Expand'} ${title}`}
                    onClick={() => toggleExpanded(row.id)}
                  >
                    <ChevronRight className="size-3" />
                  </Button>
                ) : (
                  <span className="inline-flex size-5 shrink-0" aria-hidden="true" />
                )}
                <Checkbox
                  aria-label={`Select ${title}`}
                  checked={state === 'checked'}
                  indeterminate={state === 'indeterminate'}
                  onCheckedChange={() => toggle(row.id)}
                />
                <span className="inline-flex w-[18px] shrink-0 items-center justify-center text-muted-foreground/70">{icon(row.node)}</span>
                {/* Clicking the title toggles the row, like the checkbox (the checkbox is the keyboard control). */}
                <button
                  type="button"
                  className={cn('min-w-0 flex-1 truncate text-left text-foreground', row.match && 'font-medium')}
                  tabIndex={-1}
                  onClick={() => toggle(row.id)}
                >
                  {title}
                </button>
                {row.node.child_count > 0 ? (
                  <span className="shrink-0 text-xs text-muted-foreground/70" title={`${row.node.child_count} sub-pages`}>
                    {row.node.child_count}
                  </span>
                ) : null}
              </div>
            )
          })
        )}
        {rows.length > MAX_ROWS ? (
          <p className="p-3 text-xs text-muted-foreground">
            Showing the first {MAX_ROWS} of {rows.length} rows. Filter to narrow the list.
          </p>
        ) : null}
      </div>
    </>
  )
}
