import { useMemo, useState } from 'react'
import { Database, DocumentText as FileText, Warning } from 'reicon-react'
import type { NotionImport, NotionImportNode, NotionImportTree, PageSummary, Teamspace } from '@orbit/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { Emoji } from '@/components/common/Emoji'
import { notionImportErrorMessage, useStartNotionImport } from '@/features/docs/api/notionImports'
import { buildImportTree, importSelection, selectAll, selectedCount } from '@/features/docs/import/selection'
import { useImportDestination } from '@/features/docs/import/destination'
import { ImportDestinationFields } from './ImportDestinationFields'
import { ImportTreePicker } from './ImportTreePicker'

interface Props {
  workspaceId: string
  item: NotionImport
  tree: NotionImportTree
  teamspaces: Teamspace[] | undefined
  pages: PageSummary[] | undefined
}

/** Step 3: pick pages (tri-state tree), a destination, and start the import. */
export function NotionImportChooser({ workspaceId, item, tree, teamspaces, pages }: Props) {
  const index = useMemo(() => buildImportTree(tree.nodes, (node) => [node.notion_id, node.parent_id]), [tree.nodes])
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => selectAll(index))
  const destination = useImportDestination(teamspaces, pages)
  const start = useStartNotionImport(workspaceId)
  const count = selectedCount(index, selected)

  const submit = () => {
    const selection = importSelection(index, selected)
    if (!selection) return
    start.mutate({
      importId: item.id,
      selection: 'all' in selection ? selection : { notion_ids: selection.roots },
      destination: destination.body,
    })
  }

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
      <ImportTreePicker
        tree={index}
        selected={selected}
        onSelectedChange={setSelected}
        label="Notion pages"
        empty="This token cannot see any pages. In Notion, share pages with the connection (or use a personal access token) and scan again."
        icon={(node: NotionImportNode) =>
          node.icon ? (
            <Emoji value={node.icon} />
          ) : node.kind === 'database' ? (
            <Database className="size-[15px]" aria-label="Database" />
          ) : (
            <FileText className="size-[15px]" />
          )
        }
      />
      <div className="flex flex-wrap items-end gap-3 border-t border-border px-3 py-3">
        <ImportDestinationFields destination={destination} />
        <span className="flex-1" />
        <div className="flex flex-col items-end gap-1.5">
          {start.isError ? (
            <p role="alert" className="max-w-96 text-right text-xs text-destructive">
              {notionImportErrorMessage(start.error)}
            </p>
          ) : null}
          <Button type="button" className="tabular-nums" disabled={count === 0 || start.isPending} onClick={submit}>
            Import {count} {count === 1 ? 'page' : 'pages'}
          </Button>
        </div>
      </div>
    </div>
  )
}
