import { useState } from 'react'
import { Add, ChevronDown, Copy, DocText, Edit, MoreH, Star, Trash } from 'reicon-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { ViewConfig } from '@orbit/database/types'
import { defaultTemplate, type Template } from '@orbit/database/templates'
import { useDatabaseEnv } from './context'
import { useCreateTemplate, useDeleteTemplate } from './templateApi'

interface NewRowButtonProps {
  templates: readonly Template[]
  config: ViewConfig
  onConfig: (config: ViewConfig) => void
  /** A new row from the template (`null`: an empty row), opened in the peek. */
  onNew: (templateId: string | null) => void
  /** Opens a template in the peek. */
  onEditTemplate: (templateId: string) => void
}

/** Most templates a database holds (spec P2 19). */
const MAX_TEMPLATES = 20

/**
 * "New ▾" (spec P2 7.4): New makes a row from the view's default template; the arrow lists the templates, "Empty" and
 * "New template", and each template's "⋯" edits it, makes it the view's default, duplicates or deletes it.
 */
export function NewRowButton({ templates, config, onConfig, onNew, onEditTemplate }: NewRowButtonProps) {
  const { workspaceId, databaseId, canEditViews } = useDatabaseEnv()
  const createTemplate = useCreateTemplate(workspaceId, databaseId)
  const deleteTemplate = useDeleteTemplate(workspaceId, databaseId)
  const [open, setOpen] = useState(false)
  const fallback = defaultTemplate(config, templates)
  const pick = (templateId: string | null) => {
    setOpen(false)
    onNew(templateId)
  }
  const setDefault = (templateId: string | null) => {
    const { default_template_id: _old, ...rest } = config
    onConfig(templateId ? { ...rest, default_template_id: templateId } : rest)
  }
  const addTemplate = (body: { name: string; from_row_id?: string }) =>
    createTemplate.mutate(body, {
      onSuccess: (template) => {
        setOpen(false)
        onEditTemplate(template.id)
      },
    })
  const full = templates.length >= MAX_TEMPLATES

  return (
    <div className="ml-1 flex shrink-0">
      <Button type="button" size="sm" className="rounded-r-none" onClick={() => onNew(fallback?.id ?? null)}>
        <Add />
        New
      </Button>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger
          render={
            <Button type="button" size="sm" className="rounded-l-none border-l border-primary-foreground/25 px-1.5" aria-label="New from a template" />
          }
        >
          <ChevronDown className="size-4" />
        </PopoverTrigger>
        <PopoverContent align="end" className="w-72 gap-0 p-1">
          <div className="px-2 pt-1.5 pb-1 text-xs font-medium text-muted-foreground">Templates</div>
          <ul aria-label="Templates" className="flex max-h-80 flex-col overflow-y-auto">
            {templates.map((template) => (
              <li key={template.id} className="group/template flex items-center rounded-md hover:bg-accent">
                <button
                  type="button"
                  className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-md px-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                  onClick={() => pick(template.id)}
                >
                  {template.icon ? <span className="w-4 shrink-0 text-center leading-none">{template.icon}</span> : <DocText className="size-4 shrink-0 text-muted-foreground" />}
                  <span className="truncate">{template.name}</span>
                  {fallback?.id === template.id ? (
                    <Badge variant="secondary" className="ml-auto shrink-0">
                      Default
                    </Badge>
                  ) : null}
                </button>
                <DropdownMenu>
                  <DropdownMenuTrigger
                    render={<Button type="button" variant="ghost" size="icon-xs" className="mr-1 shrink-0 text-muted-foreground" aria-label={`${template.name} options`} />}
                  >
                    <MoreH className="size-4" />
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-auto min-w-52">
                    <DropdownMenuItem
                      onClick={() => {
                        setOpen(false)
                        onEditTemplate(template.id)
                      }}
                    >
                      <Edit className="size-[14px]" />
                      Edit
                    </DropdownMenuItem>
                    {canEditViews ? (
                      <DropdownMenuItem onClick={() => setDefault(fallback?.id === template.id ? null : template.id)}>
                        <Star className="size-[14px]" />
                        {fallback?.id === template.id ? 'Remove as default' : 'Set as default for this view'}
                      </DropdownMenuItem>
                    ) : null}
                    <DropdownMenuItem disabled={full || createTemplate.isPending} onClick={() => addTemplate({ name: copyName(template.name), from_row_id: template.id })}>
                      <Copy className="size-[14px]" />
                      Duplicate
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem className="text-destructive focus:text-destructive" onClick={() => deleteTemplate.mutate(template.id)}>
                      <Trash className="size-[14px]" />
                      Delete
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </li>
            ))}
            <li className="flex items-center rounded-md hover:bg-accent">
              <button
                type="button"
                className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-md px-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                onClick={() => pick(null)}
              >
                <span className="size-4 shrink-0" aria-hidden="true" />
                <span className="truncate">Empty</span>
                {fallback ? null : (
                  <Badge variant="secondary" className="ml-auto shrink-0">
                    Default
                  </Badge>
                )}
              </button>
              {canEditViews && fallback ? (
                <Button type="button" variant="ghost" size="xs" className="mr-1 shrink-0 text-muted-foreground" onClick={() => setDefault(null)}>
                  Set as default
                </Button>
              ) : null}
            </li>
          </ul>
          <div className="my-1 h-px bg-border" />
          <button
            type="button"
            disabled={full || createTemplate.isPending}
            title={full ? 'A database holds at most 20 templates.' : undefined}
            className="flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-sm text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50"
            onClick={() => addTemplate({ name: 'New template' })}
          >
            <Add className="size-4" />
            New template
          </button>
        </PopoverContent>
      </Popover>
    </div>
  )
}

function copyName(name: string): string {
  const suffix = ' (copy)'
  return `${name.slice(0, 200 - suffix.length)}${suffix}`
}
