// Server settings › General: icon upload + server name.
import { useState, type FormEvent } from 'react'
import { updateWorkspace } from '../../mock/actions'
import { useAppState } from '../../mock/store'
import './server.css'

export function GeneralPage() {
  const state = useAppState()
  const [name, setName] = useState(state.workspace.name)
  const [saving, setSaving] = useState(false)
  const [uploadingIcon, setUploadingIcon] = useState(false)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState('')

  function flashSaved() {
    setSaved(true)
    setTimeout(() => setSaved(false), 2000)
  }

  function handleIconUpload(file?: File) {
    if (!file) return
    if (!file.type.startsWith('image/')) {
      setError('Failed to upload server icon')
      return
    }
    setUploadingIcon(true)
    setError('')
    setSaved(false)
    updateWorkspace({ iconUrl: URL.createObjectURL(file) })
    setUploadingIcon(false)
    flashSaved()
  }

  function handleSave(e: FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    setSaving(true)
    setError('')
    setSaved(false)
    updateWorkspace({ name: name.trim() })
    setSaving(false)
    flashSaved()
  }

  return (
    <div className="fs-page">
      <div>
        <h2 className="fs-title">General</h2>
        <p className="fs-subtitle">Manage your server settings</p>
      </div>

      <div>
        <label className="fs-label">Server Icon</label>
        <div className="fs-row">
          <label className="fs-icon-upload">
            <input
              type="file"
              accept="image/*"
              disabled={uploadingIcon}
              aria-label="Upload server icon"
              onChange={(e) => {
                handleIconUpload(e.target.files?.[0])
                e.currentTarget.value = ''
              }}
            />
            {state.workspace.iconUrl ? <img src={state.workspace.iconUrl} alt="" /> : state.workspace.name.charAt(0).toUpperCase()}
            {uploadingIcon ? <span className="fs-icon-upload-busy">Saving</span> : null}
          </label>
          <div className="fs-icon-hint">
            <p>Upload Icon</p>
            <p>256x256</p>
          </div>
        </div>
      </div>

      <form onSubmit={handleSave} style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        <div>
          <label className="fs-label" htmlFor="server-name">
            Server Name
          </label>
          <input id="server-name" type="text" className="fs-input" value={name} placeholder="Server name" required onChange={(e) => setName(e.target.value)} />
        </div>
        {error ? <p className="fs-error">{error}</p> : null}
        <div className="fs-row" data-end>
          <button type="submit" className="fs-btn" data-variant="primary" disabled={saving}>
            {saving ? 'Saving...' : 'Save Changes'}
          </button>
          {saved ? <span className="fs-success">Saved!</span> : null}
        </div>
      </form>
    </div>
  )
}
