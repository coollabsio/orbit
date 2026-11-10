import { useState } from 'react'
import { toast } from 'sonner'
import { Copy, DocAdd, MoreH, Trash } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Tip } from '@/components/common/Tip'
import type { DatabaseTemplate } from '@orbit/api/generated/types.gen'
import { useCreateTemplate, useDeleteTemplate, useUpdateTemplate } from './templateApi'

const MAX_NAME_CHARS = 200

/** The yellow bar over a template in the peek (spec P2 7.4), with the template's name. */
export function TemplateBar({ workspaceId, databaseId, template }: { workspaceId: string; databaseId: string; template: DatabaseTemplate }) {
  const updateTemplate = useUpdateTemplate(workspaceId, databaseId)
  const [name, setName] = useState(template.name)
  const save = () => {
    const next = name.trim()
    if (next === '' || next === template.name) return setName(template.name)
    updateTemplate.mutate({ templateId: template.id, body: { name: next } })
  }
  return (
    <div role="status" className="flex shrink-0 items-center gap-2 border-b border-amber-500/30 bg-amber-400/15 px-4 py-1.5 text-sm text-amber-900 dark:text-amber-200">
      <span className="shrink-0">You are editing a template</span>
      <span aria-hidden="true" className="text-amber-900/40 dark:text-amber-200/40">
        ·
      </span>
      <input
        aria-label="Template name"
        value={name}
        maxLength={MAX_NAME_CHARS}
        className="h-6 min-w-0 flex-1 rounded-sm bg-transparent px-1 font-medium outline-none hover:bg-amber-500/10 focus-visible:bg-background/60 focus-visible:ring-2 focus-visible:ring-amber-500/40"
        onChange={(event) => setName(event.target.value)}
        onBlur={save}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur()
          if (event.key === 'Escape') {
            setName(template.name)
            event.currentTarget.blur()
          }
        }}
      />
    </div>
  )
}

/** The peek's "⋯" for a template: duplicate it, or delete it for good. */
export function TemplateActions({ workspaceId, databaseId, template, flush, onShowRow, onClose }: {
  workspaceId: string
  databaseId: string
  template: DatabaseTemplate
  /** Saves the editor's pending edits before a copy. */
  flush: () => Promise<unknown>
  onShowRow: (rowId: string) => void
  onClose: () => void
}) {
  const createTemplate = useCreateTemplate(workspaceId, databaseId)
  const deleteTemplate = useDeleteTemplate(workspaceId, databaseId)
  const duplicate = async () => {
    await flush()
    createTemplate.mutate(
      { name: `${template.name.slice(0, MAX_NAME_CHARS - 7)} (copy)`, from_row_id: template.id },
      { onSuccess: (copy) => onShowRow(copy.id) },
    )
  }
  return (
    <DropdownMenu>
      <Tip label="Template options" side="bottom">
        <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Template options" />}>
          <MoreH className="size-4" />
        </DropdownMenuTrigger>
      </Tip>
      <DropdownMenuContent align="end" className="w-auto min-w-40">
        <DropdownMenuItem disabled={createTemplate.isPending} onClick={() => void duplicate()}>
          <Copy className="size-[14px]" />
          Duplicate
        </DropdownMenuItem>
        <DropdownMenuItem
          className="text-destructive focus:text-destructive"
          onClick={() => {
            onClose()
            deleteTemplate.mutate(template.id, { onSuccess: () => toast.success('Template deleted.') })
          }}
        >
          <Trash className="size-[14px]" />
          Delete template
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** "Save as template" in a row's "⋯": a template with the row's title, icon, values and body. */
export function SaveAsTemplateItem({ workspaceId, databaseId, rowId, title, flush, onShowRow }: {
  workspaceId: string
  databaseId: string
  rowId: string
  title: string
  flush: () => Promise<unknown>
  onShowRow: (rowId: string) => void
}) {
  const createTemplate = useCreateTemplate(workspaceId, databaseId)
  const save = async () => {
    await flush()
    const name = title.trim().slice(0, MAX_NAME_CHARS) || 'New template'
    createTemplate.mutate(
      { name, from_row_id: rowId },
      {
        onSuccess: (template) => {
          onShowRow(template.id)
          toast.success('Saved as a template.')
        },
      },
    )
  }
  return (
    <DropdownMenuItem disabled={createTemplate.isPending} onClick={() => void save()}>
      <DocAdd className="size-[14px]" />
      Save as template
    </DropdownMenuItem>
  )
}
