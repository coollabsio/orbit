// Formatting toolbar button that opens the selected image block in the full-screen ImageViewer (as in tasks and chat).
import { blockHasType, type BlockSchema, type InlineContentSchema, type StyleSchema } from '@blocknote/core'
import { useBlockNoteEditor, useComponentsContext, useEditorState } from '@blocknote/react'
import { SearchZoomIn } from 'reicon-react'
import type { Attachment } from '@/lib/attachmentLib'

export function ImageZoomButton({ onZoom }: { onZoom: (image: Attachment) => void }) {
  const Components = useComponentsContext()!
  const editor = useBlockNoteEditor<BlockSchema, InlineContentSchema, StyleSchema>()

  const block = useEditorState({
    editor,
    selector: ({ editor }) => {
      const selected = editor.getSelection()?.blocks ?? [editor.getTextCursorPosition().block]
      const block = selected.length === 1 ? selected[0] : undefined
      if (!block || block.type !== 'image' || !blockHasType(block, editor, 'image', { url: 'string', name: 'string' })) return undefined
      return block.props.url ? block : undefined
    },
  })

  if (!block) return null

  const open = async () => {
    const url = editor.resolveFileUrl ? await editor.resolveFileUrl(block.props.url) : block.props.url
    onZoom({ id: block.id, fileName: block.props.name || 'Image', mimeType: '', fileSize: 0, url })
  }

  return (
    <Components.FormattingToolbar.Button
      className="bn-button"
      label="Zoom image"
      mainTooltip="Zoom image"
      icon={<SearchZoomIn className="size-4" />}
      onClick={() => void open()}
    />
  )
}
