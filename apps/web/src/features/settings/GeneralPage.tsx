import { useRef, useState } from 'react'
import { Listbox } from '../../components/ui/Listbox'
import { UnsavedBar } from '../../components/ui/UnsavedBar'
import { useTheme, type Theme } from '../../lib/themeContext'
import { useRenameWorkspace } from '../workspaces/api'
import { useWorkspace } from '../workspaces/workspaceContext'
import { SettingsCard } from './SettingsCard'

export function GeneralPage() {
  const { theme, setTheme } = useTheme()
  const { workspace } = useWorkspace()
  const renameWorkspace = useRenameWorkspace(workspace.id)
  const formRef = useRef<HTMLFormElement>(null)
  const [nameDraft, setNameDraft] = useState<{ workspaceId: string; value: string } | null>(null)
  const name = nameDraft?.workspaceId === workspace.id ? nameDraft.value : workspace.name
  const dirty = name.trim() !== workspace.name

  function saveWorkspace() {
    if (!name.trim() || !dirty || renameWorkspace.isPending) return
    renameWorkspace.mutate(
      { name: name.trim(), version: workspace.version },
      { onSuccess: () => setNameDraft(null) },
    )
  }

  return (
    <>
      <SettingsCard title="Workspace" description="Rename this workspace.">
        <form ref={formRef} className="settings-grid" onSubmit={(event) => { event.preventDefault(); saveWorkspace() }}>
          <div className="settings-field"><label className="field-label" htmlFor="workspace-name">Name</label><input id="workspace-name" className="input" required disabled={renameWorkspace.isPending} value={name} onChange={(event) => setNameDraft({ workspaceId: workspace.id, value: event.target.value })} /></div>
        </form>
        {renameWorkspace.isError ? <p role="alert" className="text-danger">Workspace rename failed. <button className="button button-ghost" onClick={saveWorkspace}>Retry</button></p> : null}
        {renameWorkspace.isPending ? <p role="status">Saving workspace…</p> : null}
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
      {dirty ? (
        <UnsavedBar
          onReset={() => { if (renameWorkspace.isPending) return; setNameDraft(null); renameWorkspace.reset() }}
          onSave={() => formRef.current?.requestSubmit()}
          saving={renameWorkspace.isPending}
        />
      ) : null}
    </>
  )
}
