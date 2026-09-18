import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { RichTextEditor } from '../../../components/editor/RichTextEditor'
import { RichTextView } from '../../../components/editor/RichTextView'
import {
  isEmptyDocument,
  taskIdentifiersInDocument,
  toServerDocument,
  type RichTextDocument,
} from '../../../components/editor/document'
import { useTaskChips } from '../../../components/editor/useTaskChips'
import { useLatest } from '../../../lib/useLatest'
import { clipboardFiles } from '../../chat/attachmentLib'
import type { Task, TaskStatusDef, User } from '../api/models'
import { createAutosave } from './descriptionAutosave'
import { LinkifiedText } from './LinkifiedText'

const DESCRIPTION_PLACEHOLDER = 'Add description… (paste or drop images and files)'

export interface TaskTextUpdate {
  title?: string
  description_json?: RichTextDocument
  /** Set by description autosave, which tracks the version its own saves produce. */
  expected_version?: number
}

export interface TaskTextFieldsProps {
  task: Task
  workspaceId: string
  members: User[]
  statuses?: TaskStatusDef[]
  /** May return the mutation promise; a resolved `{ version }` rolls the autosave's version forward. */
  onUpdate: (update: TaskTextUpdate) => unknown
  onAttachFiles?: (files: File[]) => void
  children?: ReactNode
}

function settle(result: unknown): Promise<unknown> {
  return Promise.resolve(result)
}

export function TaskTextFields({ task, workspaceId, members, statuses = [], onUpdate, onAttachFiles, children }: TaskTextFieldsProps) {
  // Editing is tied to a task id, so switching tasks never opens the next one's editor.
  const [editingDescriptionOf, setEditingDescriptionOf] = useState<string | null>(null)
  const [dropOver, setDropOver] = useState(false)
  const editingDescription = editingDescriptionOf === task.id

  return (
    <>
      {/* Remount on a changed title so an optimistic or rejected draft never survives an authoritative value. */}
      <TitleField
        key={`${task.id}:${task.title}`}
        task={task}
        onUpdate={onUpdate}
        onTabToDescription={() => setEditingDescriptionOf(task.id)}
      />
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
        onPaste={(event) => {
          const files = clipboardFiles(event)
          if (files.length === 0) return
          event.preventDefault()
          onAttachFiles?.(files)
        }}
      >
        <DescriptionField
          key={task.id}
          task={task}
          workspaceId={workspaceId}
          members={members}
          statuses={statuses}
          editing={editingDescription}
          onEditingChange={(editing) => setEditingDescriptionOf(editing ? task.id : null)}
          onUpdate={onUpdate}
          onAttachFiles={onAttachFiles}
        />
        {children}
      </div>
    </>
  )
}

function TitleField({
  task,
  onUpdate,
  onTabToDescription,
}: {
  task: Task
  onUpdate: TaskTextFieldsProps['onUpdate']
  onTabToDescription: () => void
}) {
  const isUntitled = task.title === 'Untitled'
  const [title, setTitle] = useState(isUntitled ? '' : task.title)
  const [editing, setEditing] = useState(isUntitled)

  if (editing) {
    return (
      <input
        className="tasks-detail-title"
        value={title}
        placeholder="Task title"
        aria-label="Task title"
        autoFocus
        onChange={(event) => setTitle(event.target.value)}
        onBlur={() => {
          const value = title.trim()
          // A failed save is reported by the mutation itself.
          if (value && value !== task.title) void settle(onUpdate({ title: value })).catch(() => {})
          else if (!value) setTitle(task.title)
          setEditing(false)
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur()
          if (event.key === 'Tab' && !event.shiftKey) {
            event.preventDefault()
            event.currentTarget.blur()
            onTabToDescription()
          }
        }}
      />
    )
  }
  return (
    <div
      className="tasks-detail-title tasks-text-display"
      role="textbox"
      aria-label="Task title"
      aria-readonly="true"
      tabIndex={0}
      onClick={() => setEditing(true)}
      onKeyDown={(event) => {
        if (event.key === 'Enter') setEditing(true)
      }}
    >
      <LinkifiedText text={title} />
    </div>
  )
}

/** The newest version this field's own saves have produced. */
class VersionFloor {
  #value: number
  constructor(value: number) {
    this.#value = value
  }
  raise(value: number) {
    this.#value = Math.max(this.#value, value)
  }
  at(least: number) {
    return Math.max(this.#value, least)
  }
}

function hasVersion(value: unknown): value is { version: number } {
  return typeof value === 'object' && value !== null && typeof (value as { version?: unknown }).version === 'number'
}

/**
 * Read-only rich text until clicked; then the lazily loaded editor, saving on an
 * 800ms debounce and on blur. Saves carry the newest known version — the task's,
 * or the one the previous save returned — and never overlap, so a burst of
 * autosaves does not conflict with itself.
 */
function DescriptionField({
  task,
  workspaceId,
  members,
  statuses,
  editing,
  onEditingChange,
  onUpdate,
  onAttachFiles,
}: {
  task: Task
  workspaceId: string
  members: User[]
  statuses: TaskStatusDef[]
  editing: boolean
  onEditingChange: (editing: boolean) => void
  onUpdate: TaskTextFieldsProps['onUpdate']
  onAttachFiles?: (files: File[]) => void
}) {
  const latest = useLatest({ onUpdate, version: task.version })
  const [savedVersion] = useState(() => new VersionFloor(task.version))
  const draft = useRef<RichTextDocument | null>(null)
  const [autosave] = useState(() =>
    createAutosave(
      (description_json) => {
        const { onUpdate: update, version } = latest()
        return settle(update({ description_json, expected_version: savedVersion.at(version) })).then((result) => {
          if (hasVersion(result)) savedVersion.raise(result.version)
        })
      },
      undefined,
      toServerDocument(task.descriptionJson),
    ),
  )
  const identifiers = useMemo(() => taskIdentifiersInDocument(task.descriptionJson), [task.descriptionJson])
  const chips = useTaskChips(workspaceId, identifiers, statuses)

  // Leaving the task (or the page) mid-edit still saves what was typed.
  useEffect(
    () => () => {
      if (draft.current) autosave.flush(draft.current)
    },
    [autosave],
  )

  if (editing) {
    return (
      <RichTextEditor
        value={task.descriptionJson}
        placeholder={DESCRIPTION_PLACEHOLDER}
        ariaLabel="Description"
        autofocus
        workspaceId={workspaceId}
        members={members}
        statuses={statuses}
        onChange={(document) => {
          draft.current = document
          autosave.change(document)
        }}
        onBlur={(document) => {
          draft.current = null
          autosave.flush(document)
          onEditingChange(false)
        }}
        onCancel={() => {
          // Escape leaves editing and keeps what was typed, like Linear.
          if (draft.current) autosave.flush(draft.current)
          draft.current = null
          onEditingChange(false)
        }}
        onPasteFiles={onAttachFiles}
      />
    )
  }

  const empty = isEmptyDocument(task.descriptionJson)
  return (
    <div
      className="tasks-desc tasks-desc-view"
      data-muted={empty || undefined}
      role="textbox"
      aria-label="Description"
      aria-readonly="true"
      tabIndex={0}
      onClick={() => onEditingChange(true)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && event.target === event.currentTarget) onEditingChange(true)
      }}
    >
      {empty ? DESCRIPTION_PLACEHOLDER : <RichTextView document={task.descriptionJson} chips={chips} />}
    </div>
  )
}
