import { Suspense, lazy } from 'react'
import type { TaskStatusDef, User } from '../../features/tasks/api/models'
import type { RichTextDocument } from './document'
import type { TaskResolver } from './extensions/taskResolver'

export interface RichTextEditorProps {
  /** The document to load. Later changes apply only while the editor is not focused. */
  value: unknown
  placeholder: string
  ariaLabel: string
  compact?: boolean
  autofocus?: boolean
  workspaceId: string
  members: User[]
  /** Status definitions, so issue chips show the right status glyph. */
  statuses?: TaskStatusDef[]
  /** Overrides how a typed or pasted identifier resolves; defaults to the workspace API. */
  resolveIdentifier?: TaskResolver
  /** Every document handed out is already in the server's allowlisted shape. */
  onChange?: (document: RichTextDocument) => void
  onBlur?: (document: RichTextDocument) => void
  /** Cmd/Ctrl+Enter. */
  onSubmit?: (document: RichTextDocument) => void
  /** Escape, when the mention menu is not open. */
  onCancel?: () => void
  /** Pasted or dropped files; they never enter the document. */
  onPasteFiles?: (files: File[]) => void
}

// ProseMirror and TipTap are ~100 KB gzipped. Lists render descriptions as plain
// text, so they must never enter a list chunk — only this dynamic import pulls them in.
const importSurface = () => import('./RichTextEditorSurface')
const Surface = lazy(importSurface)

/**
 * Warm the editor chunk before it is first shown, so opening the create modal or
 * clicking a description does not stall on a multi-second first import. The dynamic
 * import is de-duped, so calling this repeatedly is free.
 */
export function prefetchRichTextEditor() {
  void importSurface()
}

export function RichTextEditor(props: RichTextEditorProps) {
  return (
    <Suspense
      fallback={
        <div
          className="editor-shell editor-shell-loading"
          data-compact={props.compact || undefined}
          aria-busy="true"
          aria-label={props.ariaLabel}
        />
      }
    >
      <Surface {...props} />
    </Suspense>
  )
}
