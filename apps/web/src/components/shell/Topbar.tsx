import { Link, useLocation, useNavigate, useSearchParams } from 'react-router'
import { Add, Menu, Moon, SearchNormal, Setting2, Sun } from 'reicon-react'
import { useTheme } from '../../lib/themeContext'
import { useAppState } from '../../mock/store'
import { createDoc } from '../../mock/actions'
import { threadTitleOf } from '../../features/chat/chatLib'
import { TaskStatusIcon } from '../workspace/TaskStatusIcon'
import { Dropdown } from '../ui/Dropdown'
import type { AppState } from '../../mock/types'

interface Crumb {
  label: string
  to?: string
}

function crumbsFor(pathname: string, folderParam: string | null, state: AppState): { crumbs: Crumb[]; status?: React.ReactNode } {
  const [, root, id, sub, subId] = pathname.split('/')
  switch (root) {
    case 'tasks': {
      const crumbs: Crumb[] = [{ label: 'Tasks', to: '/tasks' }]
      if (id === 'projects' && subId === 'settings') {
        const project = state.projects.find((p) => p.id === sub)
        if (project) crumbs.push({ label: project.name, to: `/tasks?project=${project.id}` })
        crumbs.push({ label: 'Settings' })
        return { crumbs }
      }
      const task = id ? state.tasks.find((t) => t.id === id) : null
      if (task) {
        const status = state.statuses.find((s) => s.id === task.statusId)
        const project = state.projects.find((p) => p.id === task.projectId)
        if (project) crumbs.push({ label: project.name, to: `/tasks?project=${project.id}` })
        crumbs.push({ label: task.identifier })
        return {
          crumbs,
          status: (
            <span className="topbar-status">
              <TaskStatusIcon status={status} size={12} />
              {status?.name ?? 'No status'}
            </span>
          ),
        }
      }
      return { crumbs }
    }
    case 'docs': {
      const crumbs: Crumb[] = [{ label: 'Docs', to: '/docs' }]
      let doc = id ? state.docs.find((d) => d.id === id) : null
      const chain: Crumb[] = []
      while (doc) {
        chain.unshift({ label: doc.title, to: `/docs/${doc.id}` })
        doc = doc.parentId ? state.docs.find((d) => d.id === doc?.parentId) : null
      }
      return { crumbs: [...crumbs, ...chain] }
    }
    case 'mail': {
      const crumbs: Crumb[] = [{ label: 'Mail', to: '/mail' }]
      const thread = id ? state.mailThreads.find((t) => t.id === id) : null
      const folderId = folderParam ?? thread?.folderId ?? 'f_inbox'
      const folder = state.mailFolders.find((f) => f.id === folderId)
      if (folder) crumbs.push({ label: folder.name, to: `/mail?folder=${folder.id}` })
      if (thread) crumbs.push({ label: thread.subject })
      return { crumbs }
    }
    case 'chat': {
      const crumbs: Crumb[] = [{ label: 'Chat', to: '/chat' }]
      const channel = id ? state.channels.find((c) => c.id === id) : null
      const threadRoot = channel && sub === 'thread' && subId ? state.chatMessages.find((m) => m.id === subId) : null
      if (id === 'settings') crumbs.push({ label: 'Chat Settings' })
      else if (channel && threadRoot) {
        crumbs.push({ label: `#${channel.name}`, to: `/chat/${channel.id}` }, { label: threadTitleOf(threadRoot) })
      } else if (channel) crumbs.push({ label: `#${channel.name}` })
      return { crumbs }
    }
    case 'dm': {
      const crumbs: Crumb[] = [{ label: 'Direct Messages', to: '/dm' }]
      const dm = id ? state.directMessages.find((candidate) => candidate.id === id) : null
      const participant = dm ? state.users.find((user) => user.id === dm.participantId) : null
      if (participant) crumbs.push({ label: participant.name })
      return { crumbs }
    }
    case 'inbox':
      return { crumbs: [{ label: 'Inbox' }] }
    case 'profile':
      return { crumbs: [{ label: 'Profile' }] }
    case 'settings': {
      const crumbs: Crumb[] = [{ label: 'Settings', to: '/settings' }]
      if (id === 'members') crumbs.push({ label: 'Members' })
      else if (id === 'sessions') crumbs.push({ label: 'Sessions' })
      return { crumbs }
    }
    default:
      return { crumbs: [{ label: 'Home' }] }
  }
}

export function Topbar({ onOpenDrawer, onOpenPalette }: { onOpenDrawer: () => void; onOpenPalette: () => void }) {
  const state = useAppState()
  const location = useLocation()
  const [searchParams] = useSearchParams()
  const navigate = useNavigate()
  const { theme, toggleTheme } = useTheme()

  const { crumbs, status } = crumbsFor(location.pathname, searchParams.get('folder'), state)
  const routeRoot = location.pathname.split('/')[1] || 'home'

  return (
    <header className="topbar" data-root={routeRoot}>
      <button className="icon-button topbar-menu-button topbar-drawer-button" onClick={onOpenDrawer} aria-label="Menu">
        <Menu size={18} />
      </button>
      <nav className="topbar-crumbs">
        {crumbs.map((crumb, index) => {
          const isLast = index === crumbs.length - 1
          return (
            <span key={index} style={{ display: 'contents' }}>
              {index > 0 ? <span className="topbar-crumb-sep">/</span> : null}
              {crumb.to && !isLast ? (
                <Link className="topbar-crumb" to={crumb.to}>
                  {crumb.label}
                </Link>
              ) : (
                <span className="topbar-crumb" data-current={isLast}>
                  {crumb.label}
                </span>
              )}
            </span>
          )
        })}
        {status}
      </nav>
      <div className="topbar-actions">
        <button className="icon-button topbar-menu-button" onClick={onOpenPalette} aria-label="Search">
          <SearchNormal size={16} />
        </button>
        <button className="icon-button" onClick={toggleTheme} aria-label="Toggle theme">
          {theme === 'dark' ? <Sun size={15} /> : <Moon size={15} />}
        </button>
        {routeRoot === 'home' ? (
          <button className="icon-button topbar-settings-button" onClick={() => navigate('/settings')} aria-label="Settings">
            <Setting2 size={15} />
          </button>
        ) : null}
        <Dropdown
          className="topbar-new-menu"
          align="right"
          trigger={() => (
            <span className="button button-primary">
              <Add size={16} />
              New
            </span>
          )}
        >
          {(close) => (
            <>
              <button
                className="popover-option"
                onClick={() => {
                  close()
                  navigate('/tasks?new=1')
                }}
              >
                Task
              </button>
              <button
                className="popover-option"
                onClick={() => {
                  close()
                  const doc = createDoc(null)
                  navigate(`/docs/${doc.id}`)
                }}
              >
                Document
              </button>
              <button
                className="popover-option"
                onClick={() => {
                  close()
                  navigate('/mail?compose=1')
                }}
              >
                Email
              </button>
              <button
                className="popover-option"
                onClick={() => {
                  close()
                  navigate('/chat')
                }}
              >
                Chat message
              </button>
            </>
          )}
        </Dropdown>
      </div>
    </header>
  )
}
