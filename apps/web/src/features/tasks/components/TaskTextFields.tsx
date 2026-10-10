import { lazy, Suspense, useRef, useState, type ReactNode } from 'react'
import { cva } from 'class-variance-authority'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { clipboardFiles } from '@/lib/attachmentLib'
import { renderMarkdownBlocks } from '@/lib/markdown'
import type { MentionPerson } from '@/lib/mentions'
import { normalizeMarkdown, toggleChecklistLine } from '@/components/common/markdownEditor/convert'
import type { Task } from '@/features/tasks/api/models'
import { LinkifiedText } from './LinkifiedText'

// BlockNote is large: it loads when a description is first edited
const MarkdownEditor = lazy(() => import('@/components/common/markdownEditor/MarkdownEditor'))

/**
 * The title and description share one look whether read-only, previewed or edited, so switching to the field
 * never shifts the text. `md:` repeats the sizes because Input/Textarea set their own `md:text-sm`.
 */
export const taskTextVariants = cva('w-full border-0 bg-transparent p-0 text-foreground outline-none placeholder:text-muted-foreground', {
  variants: {
    field: {
      title: 'text-2xl leading-8 font-semibold max-[899px]:text-xl max-[899px]:leading-[26px] md:text-2xl md:max-[899px]:text-xl',
      description: 'min-h-[60px] resize-none text-[13px] leading-5 [field-sizing:content] max-[899px]:min-h-12 max-[899px]:text-sm max-[899px]:leading-[22px] md:text-[13px] md:max-[899px]:text-sm',
    },
    mode: {
      read: 'whitespace-pre-wrap [overflow-wrap:anywhere]',
      // the text sits flush (p-0), so the keyboard focus ring goes outside the box rather than over the glyphs
      preview: 'cursor-text rounded-sm whitespace-pre-wrap [overflow-wrap:anywhere] focus-visible:ring-2 focus-visible:ring-ring/50 data-muted:text-muted-foreground',
      edit: 'h-auto rounded-none focus-visible:ring-0 dark:bg-transparent',
    },
  },
  // the Textarea is a flex box by default; the description field is a plain block
  compoundVariants: [{ field: 'description', mode: 'edit', className: 'block' }],
})

export function TaskTextFields({
  task,
  onUpdate,
  onAttachFiles,
  readOnly = false,
  members,
  onUploadImage,
  children,
}: {
  task: Task
  onUpdate: (update: { title?: string; description?: string }) => void
  onAttachFiles?: (files: File[]) => void
  readOnly?: boolean
  /**
   * With the members, the description opens in the rich editor (they are who "@" offers). Text that the rich editor
   * would change, and a description without this prop, is edited as plain markdown.
   */
  members?: MentionPerson[]
  /** Uploads an image that was pasted or dropped into the rich editor; it goes into the text. */
  onUploadImage?: (file: File) => Promise<string>
  children?: ReactNode
}) {
  return <TaskTextDraft key={`${task.id}:${task.version}:${task.title}:${task.description}:${readOnly}`} task={task} onUpdate={onUpdate} onAttachFiles={onAttachFiles} readOnly={readOnly} members={members} onUploadImage={onUploadImage}>{children}</TaskTextDraft>
}

function TaskTextDraft({ task, onUpdate, onAttachFiles, readOnly = false, members, onUploadImage, children }: Parameters<typeof TaskTextFields>[0]) {
  const isUntitled = task.title === 'Untitled'
  const [title, setTitle] = useState(isUntitled ? '' : task.title)
  const [description, setDescription] = useState(task.description)
  const [editingTitle, setEditingTitle] = useState(isUntitled && !readOnly)
  const [editingDescription, setEditingDescription] = useState(false)
  const [dropOver, setDropOver] = useState(false)
  // The markdown of the rich editor after the person changed the document; `null` until then. Orbit never saves a
  // description that the person did not edit: the editor's own spelling of unchanged text is not a change.
  const richDraft = useRef<string | null>(null)
  const closeRichEditor = (save: boolean) => {
    const draft = richDraft.current
    richDraft.current = null
    if (save && draft !== null && normalizeMarkdown(draft) !== normalizeMarkdown(task.description)) onUpdate({ description: draft })
    setEditingDescription(false)
  }
  const preview = (
    <EditablePreview
      className={taskTextVariants({ field: 'description', mode: 'preview' })}
      muted={!description}
      ariaLabel="Description"
      onEdit={() => setEditingDescription(true)}
    >
      {description
        ? renderMarkdownBlocks(description, 'task-description', [], {
          // a tick changes that one item and saves; the editor does not open
          onToggleChecklist: (line, checked) => {
            const next = toggleChecklistLine(task.description, line, checked)
            if (next !== null) onUpdate({ description: next })
          },
        })
        : 'Add description… (paste or drop images and files)'}
    </EditablePreview>
  )
  const textarea = (
    <Textarea
      className={taskTextVariants({ field: 'description', mode: 'edit' })}
      data-keep-font-size=""
      value={description}
      placeholder="Add description… (paste or drop images and files)"
      aria-label="Description"
      autoFocus
      onChange={(event) => setDescription(event.target.value)}
      onBlur={() => {
        if (description !== task.description) onUpdate({ description })
        setEditingDescription(false)
      }}
      onPaste={(event) => {
        const files = clipboardFiles(event)
        if (files.length === 0) return
        event.preventDefault()
        onAttachFiles?.(files)
      }}
    />
  )
  return (
    <>
      {readOnly ? (
        <div className={taskTextVariants({ field: 'title', mode: 'read' })} aria-label="Task title"><LinkifiedText text={title} /></div>
      ) : editingTitle ? (
        <Input
          className={taskTextVariants({ field: 'title', mode: 'edit' })}
          data-keep-font-size=""
          value={title}
          placeholder="Task title"
          aria-label="Task title"
          autoFocus
          onChange={(event) => setTitle(event.target.value)}
          onBlur={() => {
            const value = title.trim()
            if (value && value !== task.title) onUpdate({ title: value })
            else if (!value) setTitle(task.title)
            setEditingTitle(false)
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter') event.currentTarget.blur()
            if (event.key === 'Tab' && !event.shiftKey) {
              event.preventDefault()
              event.currentTarget.blur()
              setEditingDescription(true)
            }
          }}
        />
      ) : (
        <EditablePreview className={taskTextVariants({ field: 'title', mode: 'preview' })} ariaLabel="Task title" onEdit={() => setEditingTitle(true)}><LinkifiedText text={title} /></EditablePreview>
      )}
      <div
        className="mt-4 rounded-lg transition-[box-shadow,background-color] data-drop-over:bg-primary/10 data-drop-over:ring-2 data-drop-over:ring-primary/40"
        data-drop-over={dropOver || undefined}
        onDragOver={(event) => {
          if (!event.dataTransfer.types.includes('Files')) return
          event.preventDefault()
          setDropOver(true)
        }}
        onDragLeave={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropOver(false)
        }}
        onDrop={(event) => {
          if (!event.dataTransfer.types.includes('Files')) return
          event.preventDefault()
          setDropOver(false)
          onAttachFiles?.(Array.from(event.dataTransfer.files))
        }}
      >
        {readOnly ? (
          <div className={taskTextVariants({ field: 'description', mode: 'read' })} aria-label="Description">{description ? renderMarkdownBlocks(description, 'task-description') : 'No description'}</div>
        ) : editingDescription && members ? (
          // the read view stays until the editor's chunk is here
          <Suspense fallback={preview}>
            <MarkdownEditor
              value={task.description}
              members={members}
              ariaLabel="Description"
              className={taskTextVariants({ field: 'description', mode: 'read' })}
              autoFocus
              fallback={textarea}
              onChange={(markdown) => { richDraft.current = markdown }}
              onBlur={() => closeRichEditor(true)}
              onSubmit={() => closeRichEditor(true)}
              onCancel={() => closeRichEditor(false)}
              onFiles={onAttachFiles}
              uploadImage={onUploadImage}
            />
          </Suspense>
        ) : editingDescription ? textarea : preview}
        {children}
      </div>
    </>
  )
}

export function EditablePreview({ className, children, muted, ariaLabel, onEdit }: {
  className: string
  children: ReactNode
  muted?: boolean
  ariaLabel: string
  onEdit: () => void
}) {
  return (
    <div
      className={className}
      data-muted={muted || undefined}
      role="textbox"
      aria-label={ariaLabel}
      aria-readonly="true"
      tabIndex={0}
      onClick={(event) => {
        if (event.target instanceof Element && event.target.closest('a, button, [role=checkbox]')) return
        onEdit()
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && event.target === event.currentTarget) onEdit()
      }}
    >
      {children}
    </div>
  )
}
