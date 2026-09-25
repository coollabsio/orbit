import { useMemo, useState } from 'react'
import { ChevronRight, Database, DocumentText as FileText, Search, Warning } from 'reicon-react'
import type { NotionImport, NotionImportDestinationBody, NotionImportTree, PageSummary, Teamspace } from '@/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Emoji } from '@/components/common/Emoji'
import { notionImportErrorMessage, useStartNotionImport } from '@/features/docs/api/notionImports'
import { PRIVATE_SPACE, childrenOf, defaultTeamspace, pageTitle, rootsOf, spaceRequest, teamspaceSpace, type SpaceKey } from '@/features/docs/pageTree'
import {
  buildImportTree,
  checkStates,
  nodeTitle,
  selectAll,
  selectedCount,
  selectionBody,
  toggleNode,
  visibleRows,
} from '@/features/docs/notionImport/selection'

/** Rendering thousands of search hits at once is slow and useless; ask for a narrower query instead. */
const MAX_ROWS = 1000
const TOP_LEVEL = '__top__'

interface Props {
  workspaceId: string
  item: NotionImport
  tree: NotionImportTree
  teamspaces: Teamspace[] | undefined
  pages: PageSummary[] | undefined
}

/** Pages of one space in tree order, with their depth (the "Inside page" options). */
function spacePages(pages: readonly PageSummary[], space: SpaceKey): { page: PageSummary; depth: number }[] {
  const out: { page: PageSummary; depth: number }[] = []
  const walk = (list: PageSummary[], depth: number) => {
    for (const page of list) {
      out.push({ page, depth })
      walk(childrenOf(pages, page.id), depth + 1)
    }
  }
  walk(rootsOf(pages, space), 0)
  return out
}

/** Step 3: pick pages (tri-state tree), a destination, and start the import. */
export function NotionImportChooser({ workspaceId, item, tree, teamspaces, pages }: Props) {
  const index = useMemo(() => buildImportTree(tree.nodes), [tree.nodes])
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => selectAll(index))
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set())
  const [query, setQuery] = useState('')
  const [space, setSpace] = useState<SpaceKey | null>(null)
  const [parentId, setParentId] = useState(TOP_LEVEL)
  const start = useStartNotionImport(workspaceId)

  const states = useMemo(() => checkStates(index, selected), [index, selected])
  const rows = useMemo(() => visibleRows(index, expanded, query), [index, expanded, query])
  const count = selectedCount(index, selected)
  const fallbackTeamspace = defaultTeamspace(teamspaces)
  const destinationSpace: SpaceKey = space ?? (fallbackTeamspace ? teamspaceSpace(fallbackTeamspace.id) : PRIVATE_SPACE)
  const spaceItems = [
    ...(teamspaces ?? []).map((teamspace) => ({ value: teamspaceSpace(teamspace.id) as string, label: teamspace.name })),
    { value: PRIVATE_SPACE as string, label: 'Private' },
  ]
  const parentOptions = useMemo(() => spacePages(pages ?? [], destinationSpace), [pages, destinationSpace])
  const parentItems = [
    { value: TOP_LEVEL, label: 'Top level' },
    ...parentOptions.map(({ page }) => ({ value: page.id, label: pageTitle(page) })),
  ]
  // A page that moved away (or was trashed) is no longer a valid choice.
  const parent = parentId !== TOP_LEVEL && parentOptions.some(({ page }) => page.id === parentId) ? parentId : TOP_LEVEL

  const submit = () => {
    const selection = selectionBody(index, selected)
    if (!selection) return
    const destination: NotionImportDestinationBody = parent !== TOP_LEVEL ? { parent_page_id: parent } : spaceRequest(destinationSpace)
    start.mutate({ importId: item.id, selection, destination })
  }

  const toggleExpanded = (id: string) =>
    setExpanded((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const empty = index.nodes.length === 0

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {tree.truncated || tree.incomplete ? (
        <div role="status" className="flex flex-col gap-1 border-b border-border bg-amber-500/5 px-4 py-2 text-xs text-foreground">
          {tree.truncated ? (
            <p className="flex items-start gap-2">
              <Warning className="mt-px size-3.5 shrink-0 text-amber-600" />
              This token can see more than 5,000 pages. Only the first 5,000 are listed; import the rest in a later import.
            </p>
          ) : null}
          {tree.incomplete ? (
            <p className="flex items-start gap-2">
              <Warning className="mt-px size-3.5 shrink-0 text-amber-600" />
              Notion returned an incomplete list. Some pages may be missing; scan again later to find them.
            </p>
          ) : null}
        </div>
      ) : null}
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
        <span className="text-xs text-muted-foreground" data-testid="selected-count">
          {count} {count === 1 ? 'page' : 'pages'} selected
        </span>
        <Button type="button" variant="ghost" size="sm" onClick={() => setSelected(selectAll(index))}>
          Select all
        </Button>
        <Button type="button" variant="ghost" size="sm" onClick={() => setSelected(new Set())}>
          Clear
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2" role="tree" aria-label="Notion pages" aria-multiselectable="true">
        {empty ? (
          <p className="p-4 text-sm text-muted-foreground">
            This token cannot see any pages. In Notion, share pages with the connection (or use a personal access token) and
            scan again.
          </p>
        ) : rows.length === 0 ? (
          <p className="p-4 text-sm text-muted-foreground">No pages match “{query.trim()}”.</p>
        ) : (
          rows.slice(0, MAX_ROWS).map((row) => {
            const id = row.node.notion_id
            const state = states.get(id) ?? 'unchecked'
            const title = nodeTitle(row.node)
            return (
              <div
                key={id}
                role="treeitem"
                aria-level={row.depth + 1}
                aria-selected={state === 'checked'}
                aria-expanded={row.hasChildren ? row.expanded : undefined}
                className="flex h-8 min-w-0 items-center gap-1.5 rounded-md pr-2 text-[13px] hover:bg-sidebar-accent/50"
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
                    onClick={() => toggleExpanded(id)}
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
                  onCheckedChange={() => setSelected((current) => toggleNode(index, current, id))}
                />
                <span className="inline-flex w-[18px] shrink-0 items-center justify-center text-muted-foreground/70">
                  {row.node.icon ? (
                    <Emoji value={row.node.icon} size={15} />
                  ) : row.node.kind === 'database' ? (
                    <Database className="size-[15px]" aria-label="Database" />
                  ) : (
                    <FileText className="size-[15px]" />
                  )}
                </span>
                <button
                  type="button"
                  className={`min-w-0 flex-1 truncate text-left ${row.match ? 'font-medium text-foreground' : 'text-foreground'}`}
                  tabIndex={-1}
                  onClick={() => setSelected((current) => toggleNode(index, current, id))}
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
      <div className="flex flex-wrap items-end gap-3 border-t border-border px-3 py-3">
        <div className="grid gap-1.5">
          <Label htmlFor="notion-import-space" className="text-xs text-muted-foreground">
            Import into
          </Label>
          <Select
            items={spaceItems}
            value={destinationSpace}
            onValueChange={(value) => {
              if (!value) return
              setSpace(value as SpaceKey)
              setParentId(TOP_LEVEL)
            }}
          >
            <SelectTrigger id="notion-import-space" className="w-48" aria-label="Import into">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {spaceItems.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="grid gap-1.5">
          <Label htmlFor="notion-import-parent" className="text-xs text-muted-foreground">
            Inside page
          </Label>
          <Select items={parentItems} value={parent} onValueChange={(value) => setParentId(value ?? TOP_LEVEL)}>
            <SelectTrigger id="notion-import-parent" className="w-56" aria-label="Inside page">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={TOP_LEVEL}>Top level</SelectItem>
              {parentOptions.map(({ page, depth }) => (
                <SelectItem key={page.id} value={page.id} style={{ paddingLeft: 8 + depth * 12 }}>
                  {pageTitle(page)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <span className="flex-1" />
        <div className="flex flex-col items-end gap-1.5">
          {start.isError ? (
            <p role="alert" className="max-w-96 text-right text-xs text-destructive">
              {notionImportErrorMessage(start.error)}
            </p>
          ) : null}
          <Button type="button" disabled={count === 0 || start.isPending} onClick={submit}>
            Import {count} {count === 1 ? 'page' : 'pages'}
          </Button>
        </div>
      </div>
    </div>
  )
}
