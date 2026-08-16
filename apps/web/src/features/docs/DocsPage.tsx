import { useNavigate, useParams } from 'react-router'
import { Note2 } from 'reicon-react'
import { EmptyState } from '../../components/ui/EmptyState'
import { deleteDoc } from '../../mock/actions'
import { useAppState } from '../../mock/store'
import { DocEditor } from './components/DocEditor'
import { DocTree } from './components/DocTree'
import './docs.css'

export function DocsPage() {
  const { docId } = useParams()
  const navigate = useNavigate()
  const { docs, users } = useAppState()

  const doc = docId ? docs.find((d) => d.id === docId) : undefined

  return (
    <div className="page docs-page" data-view={docId ? 'doc' : 'index'}>
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
        <section className="pane docs-editor-pane">
          <EmptyState
            icon={Note2}
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
