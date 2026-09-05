import { useState } from 'react'
import { Listbox } from '../../components/ui/Listbox'
import { useTheme, type Theme } from '../../lib/themeContext'
import { useCreateWorkspace, useRenameWorkspace } from '../workspaces/api'
import { useWorkspace } from '../workspaces/workspaceContext'
import { SettingsCard } from './SettingsCard'

export function GeneralPage() {
  const { theme, setTheme } = useTheme()
  const { workspace, selectWorkspace } = useWorkspace()
  const renameWorkspace = useRenameWorkspace(workspace.id)
  const createWorkspace = useCreateWorkspace()
  const [nameDraft, setNameDraft] = useState({ workspaceId: workspace.id, value: workspace.name })
  const [newName, setNewName] = useState('')
  const name = nameDraft.workspaceId === workspace.id ? nameDraft.value : workspace.name
  const createAndSelectWorkspace = async (workspaceName: string) => {
    try {
      const created = await createWorkspace.mutateAsync(workspaceName)
      setNewName('')
      selectWorkspace(created.id)
    } catch {
      // The visible mutation alert retains the original name and offers retry.
    }
  }

  return (
    <>
      <SettingsCard title="Workspace" description="Rename this workspace or create another one.">
        <form className="settings-grid" onSubmit={(event) => { event.preventDefault(); renameWorkspace.mutate({ name: name.trim(), version: workspace.version }) }}>
          <div className="settings-field"><label className="field-label" htmlFor="workspace-name">Name</label><input id="workspace-name" className="input" required value={name} onChange={(event) => setNameDraft({ workspaceId: workspace.id, value: event.target.value })} /></div>
          <div className="settings-field" style={{ alignSelf: 'end' }}><button className="button button-primary" disabled={renameWorkspace.isPending || name.trim() === workspace.name}>Save workspace</button></div>
        </form>
        <form className="settings-grid" onSubmit={(event) => { event.preventDefault(); void createAndSelectWorkspace(newName.trim()) }}>
          <div className="settings-field"><label className="field-label" htmlFor="new-workspace-name">New workspace</label><input id="new-workspace-name" className="input" required value={newName} onChange={(event) => setNewName(event.target.value)} /></div>
          <div className="settings-field" style={{ alignSelf: 'end' }}><button className="button" disabled={createWorkspace.isPending}>Create workspace</button></div>
        </form>
        {renameWorkspace.isError ? <p role="alert" className="text-danger">Workspace rename failed. <button className="button button-ghost" onClick={() => renameWorkspace.variables && renameWorkspace.mutate(renameWorkspace.variables)}>Retry</button></p> : null}
        {createWorkspace.isError ? <p role="alert" className="text-danger">Workspace creation failed. <button className="button button-ghost" onClick={() => { if (createWorkspace.variables !== undefined) void createAndSelectWorkspace(createWorkspace.variables) }}>Retry</button></p> : null}
        {renameWorkspace.isPending || createWorkspace.isPending ? <p role="status">Saving workspace…</p> : null}
      </SettingsCard>

      <SettingsCard title="Appearance" description="Theme for this browser.">
        <div className="settings-grid">
          <div className="settings-field">
            <label className="field-label" htmlFor="appearance-theme">
              Theme
            </label>
            <Listbox<Theme>
              id="appearance-theme"
              value={theme}
              options={[
                { value: 'light', label: 'Light' },
                { value: 'dark', label: 'Dark' },
              ]}
              onChange={setTheme}
            />
          </div>
        </div>
      </SettingsCard>

      <SettingsCard title="About" description="Version and backend status of this Orbit instance.">
        <div className="settings-grid">
          <div className="settings-field">
            <label className="field-label" htmlFor="about-version">
              Version
            </label>
            <input id="about-version" className="input" value="0.1.0" readOnly />
          </div>
          <div className="settings-field">
            <label className="field-label" htmlFor="about-backend">
              Backend
            </label>
            <input
              id="about-backend"
              className="input"
              value="Connected"
              readOnly
            />
          </div>
          <div className="settings-field">
            <label className="field-label" htmlFor="about-storage">
              Storage
            </label>
            <input id="about-storage" className="input" value="Server data directory" readOnly />
          </div>
        </div>
      </SettingsCard>
    </>
  )
}
