import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Add } from 'reicon-react'
import { createMailFolder } from '../../../mock/actions'
import type { MailFolder, MailThread } from '../../../mock/types'
import { FOLDER_ICONS, folderUnreadCount } from '../mailLib'

interface FolderRailProps {
  folders: MailFolder[]
  threads: MailThread[]
  activeFolderId: string
}

export function FolderRail({ folders, threads, activeFolderId }: FolderRailProps) {
  const navigate = useNavigate()
  const [adding, setAdding] = useState(false)
  const [draft, setDraft] = useState('')
  const system = folders.filter((f) => !f.custom)
  const custom = folders.filter((f) => f.custom)

  const commit = () => {
    const name = draft.trim()
    setAdding(false)
    setDraft('')
    if (!name) return
    const folder = createMailFolder(name)
    navigate(`/mail?folder=${folder.id}`)
  }

  const row = (folder: MailFolder) => {
    const Icon = FOLDER_ICONS[folder.icon]
    const unread = folderUnreadCount(threads, folder.id)
    return (
      <button
        key={folder.id}
        className="menu-item"
        data-active={folder.id === activeFolderId || undefined}
        onClick={() => navigate(`/mail?folder=${folder.id}`)}
      >
        <Icon size={16} />
        <span className="menu-item-label">{folder.name}</span>
        {unread > 0 ? <span className="count-badge">{unread}</span> : null}
      </button>
    )
  }

  return (
    <nav className="pane mail-rail mail-desktop">
      <div className="pane-header">
        <span className="pane-title">Mail</span>
      </div>
      <div className="pane-body mail-rail-body">
        {system.map(row)}
        <div className="mail-rail-section">
          <span className="nav-section">Custom folders</span>
          <span className="spacer" />
          <button
            type="button"
            className="icon-button"
            aria-label="New folder"
            title="New folder"
            onClick={() => setAdding(true)}
          >
            <Add size={16} />
          </button>
        </div>
        {custom.map(row)}
        {adding ? (
          <input
            className="input mail-rail-new"
            autoFocus
            placeholder="Folder name"
            aria-label="New folder name"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                commit()
              } else if (e.key === 'Escape') {
                setDraft('')
                setAdding(false)
              }
            }}
          />
        ) : null}
      </div>
    </nav>
  )
}
