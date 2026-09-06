import { useEffect, useId, useRef, useState } from 'react'
import { Add, ChevronDown } from 'reicon-react'
import { useCreateWorkspace } from '../../features/workspaces/api'
import { useWorkspace } from '../../features/workspaces/workspaceContext'
import { Dropdown } from '../ui/Dropdown'

export function WorkspaceSwitcher({ collapsed = false, onSelect }: { collapsed?: boolean; onSelect?: () => void }) {
  const { workspace } = useWorkspace()
  const createWorkspace = useCreateWorkspace()

  return (
    <Dropdown className="workspace-switcher" trigger={(open) => (
      <button type="button" className="app-sidebar-wordmark" aria-label={`Workspace: ${workspace.name}`} aria-expanded={open} title={workspace.name}>
        <span className="app-sidebar-title">{collapsed ? workspace.name.charAt(0) : workspace.name}</span>
        {!collapsed ? <ChevronDown size={13} /> : null}
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
    <form className="workspace-create-form" onSubmit={(event) => { event.preventDefault(); void create() }}>
      <label className="field-label" htmlFor={inputId}>New workspace</label>
      <input id={inputId} className="input" autoFocus required value={name} disabled={createWorkspace.isPending} onChange={(event) => setName(event.target.value)} />
      {createWorkspace.isError ? <p role="alert" className="text-danger text-xs">Workspace creation failed. Try again.</p> : null}
      <button type="submit" className="button button-primary" disabled={!name.trim() || createWorkspace.isPending}>{createWorkspace.isPending ? 'Creating…' : 'Create workspace'}</button>
      <button type="button" className="button button-ghost" disabled={createWorkspace.isPending} onClick={() => { setCreating(false); setName(''); createWorkspace.reset() }}>Cancel</button>
    </form>
  )

  return <>
    {workspaces.map((item) => (
      <button
        key={item.id}
        type="button"
        className="popover-option"
        data-selected={item.id === workspace.id || undefined}
        aria-current={item.id === workspace.id ? 'true' : undefined}
        onClick={() => { selectWorkspace(item.id); onSelect() }}
      >
        {item.name}
      </button>
    ))}
    <div className="popover-separator" />
    <button type="button" className="popover-option" disabled={createWorkspace.isPending} onClick={() => { createWorkspace.reset(); setCreating(true) }}><Add size={15} />Create workspace</button>
  </>
}
