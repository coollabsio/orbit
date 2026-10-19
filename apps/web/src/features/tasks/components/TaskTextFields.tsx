import { useState, type ReactNode } from 'react'
import { cva } from 'class-variance-authority'
import { Input } from '@/components/ui/input'
import type { Task } from '@/features/tasks/api/models'
import { LinkifiedText } from './LinkifiedText'

/**
 * The title (and the new task dialog's fields) share one look whether read-only, previewed or edited, so switching
 * to the field never shifts the text. `md:` repeats the sizes because Input/Textarea set their own `md:text-sm`.
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
  onTitleTab,
  readOnly = false,
}: {
  task: Task
  onUpdate: (update: { title: string }) => void
  /** Tab out of the title field (focus the description). */
  onTitleTab?: () => void
  readOnly?: boolean
}) {
  return <TaskTitleDraft key={`${task.id}:${task.version}:${task.title}:${readOnly}`} task={task} onUpdate={onUpdate} onTitleTab={onTitleTab} readOnly={readOnly} />
}

function TaskTitleDraft({ task, onUpdate, onTitleTab, readOnly = false }: Parameters<typeof TaskTextFields>[0]) {
  const isUntitled = task.title === 'Untitled'
  const [title, setTitle] = useState(isUntitled ? '' : task.title)
  const [editingTitle, setEditingTitle] = useState(isUntitled && !readOnly)
  if (readOnly) {
    return <div className={taskTextVariants({ field: 'title', mode: 'read' })} aria-label="Task title"><LinkifiedText text={title} /></div>
  }
  if (!editingTitle) {
    return <EditablePreview className={taskTextVariants({ field: 'title', mode: 'preview' })} ariaLabel="Task title" onEdit={() => setEditingTitle(true)}><LinkifiedText text={title} /></EditablePreview>
  }
  return (
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
        if (event.key === 'Tab' && !event.shiftKey && onTitleTab) {
          event.preventDefault()
          event.currentTarget.blur()
          onTitleTab()
        }
      }}
    />
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
        if (event.target instanceof Element && event.target.closest('a, button')) return
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
