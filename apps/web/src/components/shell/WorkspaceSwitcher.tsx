import { useEffect, useId, useRef, useState } from 'react'
import { Plus, ChevronDown } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useCreateWorkspace } from '../../features/workspaces/api'
import { useWorkspace } from '../../features/workspaces/workspaceContext'
import { Dropdown } from '../ui/Dropdown'

const optionClass =
  'flex min-h-8 w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-accent hover:text-accent-foreground disabled:cursor-not-allowed disabled:opacity-40 max-[899px]:min-h-11 max-[899px]:whitespace-normal max-[899px]:[overflow-wrap:anywhere]'

export function WorkspaceSwitcher({ collapsed = false, onSelect }: { collapsed?: boolean; onSelect?: () => void }) {
  const { workspace } = useWorkspace()
  const createWorkspace = useCreateWorkspace()

  return (
    <Dropdown className="min-w-[13rem] p-1" trigger={(open) => (
      <button type="button" className="flex min-w-0 items-baseline gap-[7px]" aria-label={`Workspace: ${workspace.name}`} aria-expanded={open} title={workspace.name}>
        <span className="truncate text-[17px] font-bold tracking-[-0.02em] text-foreground">{collapsed ? workspace.name.charAt(0) : workspace.name}</span>
        {!collapsed ? <ChevronDown className="size-[13px] shrink-0 text-muted-foreground/70" /> : null}
      </button>
    )}>
      {(close) => <WorkspaceMenu createWorkspace={createWorkspace} onSelect={() => { close(); onSelect?.() }} />}
    </Dropdown>
  )
}

function WorkspaceMenu({ onSelect, createWorkspace }: { onSelect: () => void; createWorkspace: ReturnType<typeof useCreateWorkspace> }) {
  const { workspace, workspaces, selectWorkspace } = useWorkspace()
  const [creating, setCreating] = useState(false)
  const [name, setName] = useState('')
  const active = useRef(true)
  useEffect(() => {
    active.current = true
    return () => { active.current = false }
  }, [])
  const inputId = useId()

  const create = async () => {
    if (!name.trim() || createWorkspace.isPending) return
    try {
      const created = await createWorkspace.mutateAsync(name.trim())
      if (!active.current) return
      selectWorkspace(created.id)
      onSelect()
    } catch {
      // Keep the draft visible so the user can retry.
    }
  }

  if (creating) return (
    <form className="flex w-60 max-w-full flex-col gap-2 p-2" onSubmit={(event) => { event.preventDefault(); void create() }}>
      <label className="mb-1.5 flex h-4 items-center gap-1 text-[13px] leading-4 font-medium text-muted-foreground" htmlFor={inputId}>New workspace</label>
      <Input id={inputId} autoFocus required value={name} disabled={createWorkspace.isPending} onChange={(event) => setName(event.target.value)} />
      {createWorkspace.isError ? <p role="alert" className="text-xs text-destructive">Workspace creation failed. Try again.</p> : null}
      <Button type="submit" disabled={!name.trim() || createWorkspace.isPending}>{createWorkspace.isPending ? 'Creating…' : 'Create workspace'}</Button>
      <Button type="button" variant="ghost" disabled={createWorkspace.isPending} onClick={() => { setCreating(false); setName(''); createWorkspace.reset() }}>Cancel</Button>
    </form>
  )

  return <>
    {workspaces.map((item) => (
      <button
        key={item.id}
        type="button"
        className={cn(optionClass, item.id === workspace.id && 'bg-accent font-medium')}
        data-selected={item.id === workspace.id || undefined}
        aria-current={item.id === workspace.id ? 'true' : undefined}
        onClick={() => { selectWorkspace(item.id); onSelect() }}
      >
        {item.name}
      </button>
    ))}
    <div className="my-1 h-px bg-border" />
    <button type="button" className={optionClass} disabled={createWorkspace.isPending} onClick={() => { createWorkspace.reset(); setCreating(true) }}><Plus className="size-[15px] shrink-0" />Create workspace</button>
  </>
}
