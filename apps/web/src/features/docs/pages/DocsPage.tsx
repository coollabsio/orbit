import { useNavigate, useParams } from 'react-router'
import { DocumentText as FileText } from 'reicon-react'
import { EmptyState } from '@/components/common/EmptyState'
import { deleteDoc } from '@/mock/actions'
import { useAppState } from '@/mock/store'
import { DocEditor } from '@/features/docs/components/DocEditor'
import { DocTree } from '@/features/docs/components/DocTree'

export function DocsPage() {
  const { docId } = useParams()
  const navigate = useNavigate()
  const { docs, users } = useAppState()

  const doc = docId ? docs.find((d) => d.id === docId) : undefined

  return (
    <div className="group/docs flex min-h-0 min-w-0 flex-1 overflow-hidden bg-card" data-view={docId ? 'doc' : 'index'}>
      <DocTree docs={docs} activeId={docId ?? null} />
      {doc ? (
        <DocEditor
          key={doc.id}
          doc={doc}
          docs={docs}
          users={users}
          onDelete={() => {
            deleteDoc(doc.id)
            navigate('/docs')
          }}
        />
      ) : (
        <section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background max-[899px]:group-data-[view=index]/docs:hidden">
          <EmptyState
            icon={FileText}
            title={docId ? 'Document not found' : 'Select a document'}
            description={
              docId
                ? 'This page may have been deleted.'
                : 'Choose a page from the sidebar or create a new one.'
            }
          />
        </section>
      )}
    </div>
  )
}
