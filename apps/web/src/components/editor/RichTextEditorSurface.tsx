import { useQueryClient, type QueryClient } from '@tanstack/react-query'
import { EditorContent, ReactNodeViewRenderer, useEditor } from '@tiptap/react'
import { useEffect, useLayoutEffect, useMemo, useState } from 'react'
import { queryKeys } from '../../api/queryKeys'
import { editableDocument, sameDocument, taskIdentifiersInDocument, toServerDocument } from './document'
import { buildEditorExtensions } from './extensions/editorExtensions'
import { createTaskResolver, type TaskResolver } from './extensions/taskResolver'
import { TaskMention } from './extensions/taskMention'
import type { RichTextEditorProps } from './RichTextEditor'
import { TaskChipContext } from './taskChipContext'
import { TaskMentionNodeView } from './TaskMentionNodeView'
import { useTaskChips } from './useTaskChips'
import './editor.css'

const TaskMentionWithView = TaskMention.extend({
  addNodeView() {
    return ReactNodeViewRenderer(TaskMentionNodeView, { as: 'span' })
  },
})

/** Reads the newest props, for handlers that outlive the render that created them. */
type Latest = () => RichTextEditorProps

/**
 * Holds the props of the latest commit. The editor and its extensions are built
 * once, so their handlers read callbacks and members through this at event time
 * instead of closing over the render that created them.
 */
class PropsBridge {
  #props: RichTextEditorProps
  constructor(props: RichTextEditorProps) {
    this.#props = props
  }
  update(props: RichTextEditorProps) {
    this.#props = props
  }
  read: Latest = () => this.#props
}

/**
 * Built once per editor. Everything that can change between renders — members,
 * callbacks — is read through `latest` at event time.
 */
function surfaceExtensions(
  latest: Latest,
  {
    placeholder,
    workspaceId,
    resolve,
    queryClient,
  }: { placeholder: string; workspaceId: string; resolve?: TaskResolver; queryClient: QueryClient },
) {
  return buildEditorExtensions({
    placeholder,
    taskMention: TaskMentionWithView,
    resolve: resolve ?? createTaskResolver(workspaceId),
    mentions: {
      workspaceId,
      members: () => latest().members,
      // A picked issue is known already: seed its chip so the title shows at once.
      onPickTask: (task) =>
        queryClient.setQueryData(queryKeys.taskChips(workspaceId, task.identifier), {
          taskId: task.id,
          title: task.title,
          statusId: task.status_id,
        }),
    },
    keys: {
      submit: (document) => {
        const { onSubmit } = latest()
        if (!onSubmit) return false
        onSubmit(toServerDocument(document))
        return true
      },
      cancel: () => {
        const { onCancel } = latest()
        if (!onCancel) return false
        onCancel()
        return true
      },
    },
  })
}

function filesFrom(data: DataTransfer | null): File[] {
  return Array.from(data?.files ?? [])
}

export default function RichTextEditorSurface(props: RichTextEditorProps) {
  const { value, placeholder, ariaLabel, compact, autofocus, workspaceId, statuses = [] } = props
  // Callbacks and members are read at event time, so the editor is built once
  // and never torn down because a parent re-rendered with new closures.
  const [bridge] = useState(() => new PropsBridge(props))
  useLayoutEffect(() => bridge.update(props))
  const latest = bridge.read

  const queryClient = useQueryClient()
  const identifiers = useMemo(() => taskIdentifiersInDocument(value), [value])
  const chips = useTaskChips(workspaceId, identifiers, statuses)

  const [extensions] = useState(() =>
    surfaceExtensions(latest, { placeholder, workspaceId, resolve: props.resolveIdentifier, queryClient }),
  )

  const editor = useEditor({
    extensions,
    content: editableDocument(value),
    autofocus: autofocus ? 'end' : false,
    editorProps: {
      attributes: {
        class: 'editor-surface',
        role: 'textbox',
        'aria-label': ariaLabel,
        'aria-multiline': 'true',
      },
      // Files become attachments, never document content. Propagation stops so a
      // surrounding drop/paste zone does not attach the same files twice.
      handlePaste: (_view, event) => {
        const files = filesFrom(event.clipboardData)
        const { onPasteFiles } = latest()
        if (files.length === 0 || !onPasteFiles) return false
        event.stopPropagation()
        onPasteFiles(files)
        return true
      },
      handleDrop: (_view, event) => {
        const files = filesFrom(event.dataTransfer)
        const { onPasteFiles } = latest()
        if (files.length === 0 || !onPasteFiles) return false
        event.stopPropagation()
        onPasteFiles(files)
        return true
      },
    },
    onUpdate: ({ editor: current }) => latest().onChange?.(toServerDocument(current.getJSON())),
    onBlur: ({ editor: current }) => latest().onBlur?.(toServerDocument(current.getJSON())),
  })

  // An authoritative refresh (another client's write, a conflict resolution)
  // replaces the content — but never while the user is typing in it, where the
  // local draft is the newer truth and the autosave carries it.
  useEffect(() => {
    if (!editor || editor.isDestroyed || editor.isFocused) return
    if (!sameDocument(editor.getJSON(), value)) {
      editor.commands.setContent(editableDocument(value), { emitUpdate: false })
    }
  }, [editor, value])

  return (
    <TaskChipContext value={chips}>
      <EditorContent className="editor-shell" data-compact={compact || undefined} editor={editor} />
    </TaskChipContext>
  )
}
