import { useNavigate } from 'react-router'
import type { MailFolder, MailThread } from '../../../mock/types'
import { FOLDER_ICONS, folderUnreadCount } from '../mailLib'

interface FolderRailProps {
  folders: MailFolder[]
  threads: MailThread[]
  activeFolderId: string
}

export function FolderRail({ folders, threads, activeFolderId }: FolderRailProps) {
  const navigate = useNavigate()
  return (
    <nav className="pane mail-rail mail-desktop">
      <div className="nav-section">Folders</div>
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
