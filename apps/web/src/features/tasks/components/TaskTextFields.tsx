import { useState, type ReactNode } from 'react'
import { cn } from 'cn'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { clipboardFiles } from '@/lib/attachmentLib'
import { renderMarkdownBlocks } from '@/lib/markdown'
import type { Task } from '@/features/tasks/api/models'
import { LinkifiedText } from './LinkifiedText'

const TITLE = 'w-full border-none bg-transparent p-0 text-2xl leading-8 font-semibold text-foreground outline-none placeholder:text-muted-foreground max-[899px]:text-xl max-[899px]:leading-[26px]'
const DESC = 'w-full resize-none border-none bg-transparent p-0 text-[13px] leading-5 text-foreground outline-none [field-sizing:content] min-h-[60px] placeholder:text-muted-foreground max-[899px]:min-h-12 max-[899px]:text-sm max-[899px]:leading-[22px]'
const DISPLAY = 'cursor-text whitespace-pre-wrap [overflow-wrap:anywhere] data-[muted]:text-muted-foreground'

export function TaskTextFields({
  task,
  onUpdate,
  onAttachFiles,
  readOnly = false,
  children,
}: {
  task: Task
  onUpdate: (update: { title?: string; description?: string }) => void
  onAttachFiles?: (files: File[]) => void
  readOnly?: boolean
  children?: ReactNode
}) {
  return <TaskTextDraft key={`${task.id}:${task.version}:${task.title}:${task.description}:${readOnly}`} task={task} onUpdate={onUpdate} onAttachFiles={onAttachFiles} readOnly={readOnly}>{children}</TaskTextDraft>
}

function TaskTextDraft({ task, onUpdate, onAttachFiles, readOnly = false, children }: Parameters<typeof TaskTextFields>[0]) {
  const isUntitled = task.title === 'Untitled'
  const [title, setTitle] = useState(isUntitled ? '' : task.title)
  const [description, setDescription] = useState(task.description)
  const [editingTitle, setEditingTitle] = useState(isUntitled && !readOnly)
  const [editingDescription, setEditingDescription] = useState(false)
  const [dropOver, setDropOver] = useState(false)
  return (
    <>
      {readOnly ? (
        <div className={`${TITLE} whitespace-pre-wrap [overflow-wrap:anywhere]`} aria-label="Task title"><LinkifiedText text={title} /></div>
      ) : editingTitle ? (
        <Input
          className={cn(TITLE, 'h-auto rounded-none border-0 shadow-none focus-visible:ring-0 dark:bg-transparent md:text-2xl md:max-[899px]:text-xl')}
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
        <EditablePreview className={`${TITLE} ${DISPLAY}`} ariaLabel="Task title" onEdit={() => setEditingTitle(true)}><LinkifiedText text={title} /></EditablePreview>
      )}
      <div
        className="mt-4 rounded-lg transition-[box-shadow,background-color] data-[drop-over]:bg-primary/10 data-[drop-over]:ring-2 data-[drop-over]:ring-primary/40"
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
          <div className={`${DESC} [overflow-wrap:anywhere]`} aria-label="Description">{description ? renderMarkdownBlocks(description, 'task-description') : 'No description'}</div>
        ) : editingDescription ? (
          <Textarea
            className={cn(DESC, 'block rounded-none border-0 shadow-none focus-visible:ring-0 dark:bg-transparent md:text-[13px] md:max-[899px]:text-sm')}
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
        ) : (
          <EditablePreview
            className={`${DESC} ${DISPLAY}`}
            muted={!description}
            ariaLabel="Description"
            onEdit={() => setEditingDescription(true)}
          >{description ? renderMarkdownBlocks(description, 'task-description') : 'Add description… (paste or drop images and files)'}</EditablePreview>
        )}
        {children}
      </div>
    </>
  )
}

function EditablePreview({ className, children, muted, ariaLabel, onEdit }: {
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
