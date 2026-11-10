import { Link } from 'react-router'
import { ClipboardText } from 'reicon-react'
import { buttonVariants } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { EmptyState } from '@/components/common/EmptyState'
import { formConfig } from '@orbit/database/form'
import { repairConfig } from '@orbit/database/view'
import { useDatabase } from './api'
import { DatabaseEnvContext, useDatabaseEnvValue } from './context'
import { FormFill } from './DatabaseForm'

/** The full-page form of a form view (`/docs/<database>/form/<view>`, spec P3 18.2), for members who can open the database. */
export function DatabaseFormPage({ workspaceId, databaseId, viewId }: { workspaceId: string; databaseId: string; viewId: string }) {
  const query = useDatabase(workspaceId, databaseId)
  const database = query.data
  const env = useDatabaseEnvValue(workspaceId, databaseId, database?.database.locked ?? true)
  const view = database?.views.find((candidate) => candidate.id === viewId && candidate.type === 'form')

  let content
  if (database && view) {
    content = <FormFill database={database} viewId={view.id} form={formConfig(repairConfig(view, database.properties), database.properties)} />
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
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto px-6 pt-12 pb-24 max-[899px]:px-3 max-[899px]:pt-6">{content}</div>
    </DatabaseEnvContext.Provider>
  )
}
