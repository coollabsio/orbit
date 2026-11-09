import type { ReactNode } from 'react'
import { ArrowUpRight, Grid } from 'reicon-react'
import { ApiProblem } from '@orbit/api/problem'
import { ISOLATED_ATTR } from '@orbit/editor/isolation'
import type { BlockNoteEditor } from '@blocknote/core'
import { Emoji } from '@/components/common/Emoji'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { usePageTrash } from '../api/pages'
import { useDatabase } from '../database/api'
import { useDatabaseEnvValue } from '../database/context'
import { DatabaseSurface } from '../database/DatabaseSurface'
import { usePageEditorContext, type DatabaseBlockHost } from './pageEditorContext'

/** Rows a table in a page shows before "Load more", and cards per board column before "Show more" (spec 3.5). */
export const BLOCK_ROW_LIMIT = 25

type DatabaseBlockProps = { databaseId: string; linkId: string }
type DatabaseBlockViewProps = { block: { id: string; props: DatabaseBlockProps }; editor: Pick<BlockNoteEditor<any, any, any>, 'updateBlock'> }

const ISOLATED = { [ISOLATED_ATTR]: '' }

/**
 * `render` of the `database` block spec: a database inside a page (spec 3.5). Its root is `contentEditable={false}`
 * and isolated, so keys, pastes and drags in the table never reach the editor; the block is selected as a whole from
 * its margin or with the arrow keys, and Backspace deletes it then.
 */
export function DatabaseBlockView({ block, editor }: DatabaseBlockViewProps) {
  const { databaseHost } = usePageEditorContext()
  const { databaseId, linkId } = block.props
  return (
    // The app font and text settings, not the editor's (BlockNote's font, ProseMirror's ligatures and word breaks).
    <div
      {...ISOLATED}
      contentEditable={false}
      data-slot="database-block"
      data-database-id={databaseId}
      className="flex w-full min-w-0 flex-col py-1 font-sans text-sm wrap-normal [font-feature-settings:normal] [font-variant-ligatures:normal]"
    >
      {!databaseId ? (
        <Notice>Database not found</Notice>
      ) : !databaseHost ? (
        <DatabaseTitle databaseId={databaseId} linked={!!linkId} />
      ) : (
        <DatabaseBlockBody
          host={databaseHost}
          blockId={block.id}
          databaseId={databaseId}
          linkId={linkId}
          onShowOwnViews={() => editor.updateBlock(block.id, { props: { linkId: '' } })}
        />
      )}
    </div>
  )
}

/** `toExternalHTML`: a link to the database, as a Markdown export writes it. */
export function DatabaseBlockExternalView({ block }: { block: { props: DatabaseBlockProps } }) {
  const { resolvePage } = usePageEditorContext()
  const title = resolvePage(block.props.databaseId)?.title.trim() || 'Untitled'
  return (
    <p>
      <a href={`/docs/${block.props.databaseId}`}>{title}</a>
    </p>
  )
}

function DatabaseBlockBody({ host, blockId, databaseId, linkId, onShowOwnViews }: { host: DatabaseBlockHost; blockId: string; databaseId: string; linkId: string; onShowOwnViews: () => void }) {
  const query = useDatabase(host.workspaceId, databaseId)
  // Without a link this is the same query (one request, one cache entry).
  const viewsQuery = useDatabase(host.workspaceId, databaseId, linkId || null)
  const database = query.data
  const env = useDatabaseEnvValue(host.workspaceId, databaseId, database?.database.locked ?? true, linkId ? { id: linkId, hostLocked: host.locked } : undefined)

  if (query.error) {
    return query.error instanceof ApiProblem && query.error.status === 404 ? (
      <MissingDatabase workspaceId={host.workspaceId} databaseId={databaseId} />
    ) : (
      <Notice>
        The database could not load.
        <Button type="button" variant="outline" size="xs" onClick={() => void query.refetch()}>
          Retry
        </Button>
      </Notice>
    )
  }
  if (viewsQuery.error) {
    const gone = viewsQuery.error instanceof ApiProblem && viewsQuery.error.code === 'database_link_not_found'
    return (
      <>
        <DatabaseTitle databaseId={databaseId} linked />
        <Notice>
          {gone ? 'View not found' : 'The views could not load.'}
          <Button type="button" variant="outline" size="xs" onClick={gone ? onShowOwnViews : () => void viewsQuery.refetch()}>
            {gone ? "Show the database's own views" : 'Retry'}
          </Button>
        </Notice>
      </>
    )
  }
  if (!database || !viewsQuery.data) {
    return (
      <div className="flex h-9 items-center px-1">
        <Spinner className="text-muted-foreground" />
      </div>
    )
  }
  return (
    <DatabaseSurface
      database={database}
      views={viewsQuery.data.views}
      env={env}
      locked={database.database.locked}
      viewKey={`orbit:database-view:${databaseId}:${blockId}`}
      block={{ id: blockId, rowLimit: BLOCK_ROW_LIMIT }}
      layout={({ controls, content }) => (
        <>
          <DatabaseTitle databaseId={databaseId} linked={!!linkId} title={database.database.title} icon={database.database.icon} />
          {controls}
          {/* The table starts at the text edge. The row handles and checkboxes (40 px) hang in the page margin, inside this
              scroller. Below 900 px the page has no margin for them: they hide. */}
          <div className="-ml-10 overflow-x-auto overflow-y-hidden pt-2 pb-1 pl-10 max-[899px]:ml-0 max-[899px]:pl-0 max-[899px]:[&_[data-slot=row-gutter]]:hidden">
            {content}
          </div>
        </>
      )}
    />
  )
}

/** The database's icon and title (a click opens its full page); "↗" marks a linked view. */
function DatabaseTitle({ databaseId, linked, title, icon }: { databaseId: string; linked: boolean; title?: string; icon?: string | null }) {
  const { resolvePage, onOpenPage } = usePageEditorContext()
  const page = resolvePage(databaseId)
  const name = (title ?? page?.title ?? '').trim() || 'Untitled'
  const emoji = icon !== undefined ? icon : page?.icon
  return (
    <button
      type="button"
      className="flex w-fit max-w-full min-w-0 items-center gap-1.5 rounded-md px-1 py-1 text-left text-base font-semibold text-foreground outline-none hover:bg-foreground/[0.04] focus-visible:ring-2 focus-visible:ring-ring/50"
      onClick={() => onOpenPage(databaseId)}
    >
      <span className="flex size-5 shrink-0 items-center justify-center" aria-hidden>
        {emoji ? <Emoji value={emoji} /> : <Grid className="size-[18px] text-muted-foreground" />}
      </span>
      <span className="truncate">{name}</span>
      {linked ? <ArrowUpRight className="size-3.5 shrink-0 text-muted-foreground" aria-label="Linked view" /> : null}
    </button>
  )
}

/** A 404: the database is in the trash, or the viewer cannot read it (then its title stays hidden). */
function MissingDatabase({ workspaceId, databaseId }: { workspaceId: string; databaseId: string }) {
  const trash = usePageTrash(workspaceId)
  const trashed = trash.data?.some((item) => item.id === databaseId)
  return <Notice>{trashed ? 'Database in trash' : 'No access to this database'}</Notice>
}

function Notice({ children }: { children: ReactNode }) {
  return <div className="flex items-center gap-2 px-1 py-1.5 text-sm text-muted-foreground">{children}</div>
}
