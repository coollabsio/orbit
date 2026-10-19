import { lazy } from 'react'
import { Spinner } from '@/components/ui/spinner'
import { loadPageEditor } from '@/features/docs/editor/loadPageEditor'

/** The BlockNote editor (task profile for task text), loaded on demand: its chunk is about 1 MB. */
export const PageEditor = lazy(() => loadPageEditor().then((module) => ({ default: module.PageEditor })))

export function EditorFallback() {
  return (
    <div className="flex justify-center py-4">
      <Spinner className="text-muted-foreground" />
    </div>
  )
}

/** An image or file block for an uploaded file. */
export function fileBlock(file: File, url: string) {
  return { type: file.type.startsWith('image/') ? 'image' : 'file', props: { url, name: file.name } }
}
