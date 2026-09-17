import { useState, type ReactNode } from 'react'
import { clipboardFiles } from '../../chat/attachmentLib'
import type { Task } from '../api/models'
import { LinkifiedText } from './LinkifiedText'

export function TaskTextFields({
  task,
  onUpdate,
  onAttachFiles,
  children,
}: {
  task: Task
  onUpdate: (update: { title?: string; description?: string }) => void
  onAttachFiles?: (files: File[]) => void
  children?: ReactNode
}) {
  return <TaskTextDraft key={`${task.id}:${task.version}:${task.title}:${task.description}`} task={task} onUpdate={onUpdate} onAttachFiles={onAttachFiles}>{children}</TaskTextDraft>
}

function TaskTextDraft({ task, onUpdate, onAttachFiles, children }: Parameters<typeof TaskTextFields>[0]) {
  const [title, setTitle] = useState(task.title)
  const [description, setDescription] = useState(task.description)
  const [editingTitle, setEditingTitle] = useState(task.title === 'Untitled')
  const [editingDescription, setEditingDescription] = useState(false)
  const [dropOver, setDropOver] = useState(false)
  return (
    <>
      {editingTitle ? (
        <input
          className="tasks-detail-title"
          value={title}
          placeholder="Task title"
          aria-label="Task title"
          autoFocus
          onChange={(event) => setTitle(event.target.value)}
          onBlur={() => {
            const value = title.trim()
            if (value && value !== task.title) onUpdate({ title: value })
            setEditingTitle(false)
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter') event.currentTarget.blur()
          }}
        />
      ) : (
        <EditableLinkifiedText className="tasks-detail-title tasks-text-display" text={title} ariaLabel="Task title" onEdit={() => setEditingTitle(true)} />
      )}
      <div
        className="tasks-desc-wrap"
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
        {editingDescription ? (
          <textarea
            className="tasks-desc"
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
          <EditableLinkifiedText
            className="tasks-desc tasks-text-display"
            text={description || 'Add description… (paste or drop images and files)'}
            muted={!description}
            ariaLabel="Description"
            onEdit={() => setEditingDescription(true)}
          />
        )}
        {children}
      </div>
    </>
  )
}

function EditableLinkifiedText({ className, text, muted, ariaLabel, onEdit }: {
  className: string
  text: string
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
      onClick={onEdit}
      onKeyDown={(event) => {
        if (event.key === 'Enter') onEdit()
      }}
    >
      <LinkifiedText text={text} />
    </div>
  )
}
