import { useMemo, useState } from 'react'
import { DocumentText as FileText, Folder } from 'reicon-react'
import type { ObsidianImport, ObsidianImportNode, ObsidianImportTree, PageSummary, Teamspace } from '@orbit/api/generated/types.gen'
import { Button } from '@/components/ui/button'
import { obsidianImportErrorMessage, skippedSummary, useStartObsidianImport } from '@/features/docs/api/obsidianImports'
import { buildImportTree, importSelection, selectAll, selectedCount } from '@/features/docs/import/selection'
import { useImportDestination } from '@/features/docs/import/destination'
import { ImportDestinationFields } from './ImportDestinationFields'
import { ImportTreePicker } from './ImportTreePicker'

interface Props {
  workspaceId: string
  item: ObsidianImport
  tree: ObsidianImportTree
  teamspaces: Teamspace[] | undefined
  pages: PageSummary[] | undefined
}

/** Pick notes and folders (tri-state tree), a destination, and start the import. */
export function ObsidianImportChooser({ workspaceId, item, tree, teamspaces, pages }: Props) {
  const index = useMemo(() => buildImportTree(tree.nodes, (node) => [node.path, node.parent_path]), [tree.nodes])
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => selectAll(index))
  const destination = useImportDestination(teamspaces, pages)
  const start = useStartObsidianImport(workspaceId)
  const count = selectedCount(index, selected)
  const skipped = skippedSummary(tree.skipped)

  const submit = () => {
    const selection = importSelection(index, selected)
    if (!selection) return
    start.mutate({
      importId: item.id,
      selection: 'all' in selection ? selection : { paths: selection.roots },
      destination: destination.body,
    })
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div role="status" aria-label="Vault contents" className="flex flex-col gap-1 border-b border-border px-4 py-2 text-xs text-muted-foreground">
        <p>
          {tree.attachments} {tree.attachments === 1 ? 'attachment' : 'attachments'} ·{' '}
          {tree.unresolved_links} unresolved {tree.unresolved_links === 1 ? 'link' : 'links'} (kept as text) · {skipped.total} skipped{' '}
          {skipped.total === 1 ? 'file' : 'files'}
        </p>
        {skipped.parts.length > 0 ? <p>Skipped: {skipped.parts.join(', ')}</p> : null}
      </div>
      <ImportTreePicker
        tree={index}
        selected={selected}
        onSelectedChange={setSelected}
        label="Obsidian notes"
        empty="The vault has no notes to import."
        icon={(node: ObsidianImportNode) =>
          node.kind === 'folder' ? <Folder className="size-[15px]" aria-label="Folder" /> : <FileText className="size-[15px]" />
        }
      />
      <div className="flex flex-wrap items-end gap-3 border-t border-border px-3 py-3">
        <ImportDestinationFields destination={destination} />
        <span className="flex-1" />
        <div className="flex flex-col items-end gap-1.5">
          {start.isError ? (
            <p role="alert" className="max-w-96 text-right text-xs text-destructive">
              {obsidianImportErrorMessage(start.error)}
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
