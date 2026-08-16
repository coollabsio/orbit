import { Edit } from 'reicon-react'
import { useNavigate } from 'react-router'
import type { MailFolder, MailThread } from '../../../mock/types'
import { FOLDER_ICONS, folderUnreadCount } from '../mailLib'

interface FolderRailProps {
  folders: MailFolder[]
  threads: MailThread[]
  activeFolderId: string
  onCompose: () => void
}

export function FolderRail({ folders, threads, activeFolderId, onCompose }: FolderRailProps) {
  const navigate = useNavigate()
  return (
    <nav className="pane mail-rail mail-desktop">
      <div className="pane-header">
        <span className="pane-title">Mail</span>
        <span className="spacer" />
        <button className="icon-button" aria-label="Compose" onClick={onCompose}>
          <Edit size={16} />
        </button>
      </div>
      <div className="pane-body mail-rail-body">
        {folders.map((folder) => {
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
        })}
      </div>
    </nav>
  )
}
