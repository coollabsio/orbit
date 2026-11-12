import { Link, useLocation, useSearchParams } from 'react-router'
import { ArrowLeft, ClipboardText } from 'reicon-react'
import { buttonVariants } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { EmptyState } from '@/components/common/EmptyState'
import { PaneHeader, PaneTitle } from '@/components/common/Pane'
import { formConfig } from '@orbit/database/form'
import { repairConfig } from '@orbit/database/view'
import { useDatabase } from './api'
import { DatabaseEnvContext, useDatabaseEnvValue } from './context'
import { FormFill, type FormPageState } from './DatabaseForm'

/**
 * The full-page form of a form view (`/docs/<database>/form/<view>`, spec P3 18.2), for members who can open the
 * database. `?link=<id>` reads the view from a linked view block's views instead of the database's own.
 */
export function DatabaseFormPage({ workspaceId, databaseId, viewId }: { workspaceId: string; databaseId: string; viewId: string }) {
  const [searchParams] = useSearchParams()
  const linkId = searchParams.get('link')
  const back = (useLocation().state as FormPageState | null)?.back ?? `/docs/${databaseId}`
  const query = useDatabase(workspaceId, databaseId, linkId)
  const database = query.data
  const env = useDatabaseEnvValue(workspaceId, databaseId, database?.database.locked ?? true, linkId ? { id: linkId, hostLocked: true } : undefined)
  const view = database?.views.find((candidate) => candidate.id === viewId && candidate.type === 'form')
  const form = database && view ? formConfig(repairConfig(view, database.properties), database.properties) : null
  const databaseTitle = database?.database.title || 'Untitled'

  let content
  if (database && form) {
    content = <FormFill database={database} viewId={viewId} form={form} />
  } else if (query.isPending) {
    content = <Spinner className="mx-auto text-muted-foreground" />
  } else {
    content = (
      <EmptyState
        icon={ClipboardText}
        title={query.isError ? 'Form unavailable' : 'Form not found'}
        description={query.isError ? 'The form could not load. You need access to its database.' : 'This form was deleted.'}
        action={
          <Link className={buttonVariants({ variant: 'outline' })} to={database ? `/docs/${databaseId}` : '/docs'}>
            {database ? 'Open the database' : 'Back to Docs'}
          </Link>
        }
      />
    )
  }
  return (
    <DatabaseEnvContext.Provider value={env}>
      <PaneHeader>
        <Link className={buttonVariants({ variant: 'ghost', size: 'icon-sm', className: 'text-muted-foreground/70' })} to={back} aria-label="Back">
          <ArrowLeft className="size-4" />
        </Link>
        <nav aria-label="Breadcrumb" className="flex min-w-0 flex-1 items-center gap-1.5 text-[0.8125rem]">
          {database ? (
            <Link className="min-w-0 truncate text-muted-foreground hover:text-foreground" to={`/docs/${databaseId}`}>
              {databaseTitle}
            </Link>
          ) : null}
          {database ? <span className="text-muted-foreground/60" aria-hidden="true">/</span> : null}
          <PaneTitle aria-current="page">{form?.title || view?.name || 'Form'}</PaneTitle>
        </nav>
      </PaneHeader>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto px-6 pt-12 pb-24 max-[899px]:px-3 max-[899px]:pt-6">{content}</div>
    </DatabaseEnvContext.Provider>
  )
}
