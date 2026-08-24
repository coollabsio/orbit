import { Link, useLocation, useNavigate, useSearchParams } from 'react-router'
import { Add, Menu, Moon, SearchNormal, Sun } from 'reicon-react'
import { useTheme } from '../../lib/themeContext'
import { useAppState } from '../../mock/store'
import { createDoc } from '../../mock/actions'
import { STATUS_LABEL } from '../workspace/taskMeta'
import { TaskStatusIcon } from '../workspace/TaskStatusIcon'
import { Dropdown } from '../ui/Dropdown'
import type { AppState } from '../../mock/types'

interface Crumb {
  label: string
  to?: string
}

function crumbsFor(pathname: string, folderParam: string | null, state: AppState): { crumbs: Crumb[]; status?: React.ReactNode } {
  const [, root, id] = pathname.split('/')
  switch (root) {
    case 'tasks': {
      const crumbs: Crumb[] = [{ label: 'Tasks', to: '/tasks' }]
      const task = id ? state.tasks.find((t) => t.id === id) : null
      if (task) {
        const project = state.projects.find((p) => p.id === task.projectId)
        if (project) crumbs.push({ label: project.name, to: `/tasks?project=${project.id}` })
        crumbs.push({ label: task.identifier })
        return {
          crumbs,
          status: (
            <span className="topbar-status">
              <TaskStatusIcon status={task.status} size={12} />
              {STATUS_LABEL[task.status]}
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
      if (channel) crumbs.push({ label: `#${channel.name}` })
      return { crumbs }
    }
    case 'inbox':
      return { crumbs: [{ label: 'Inbox' }] }
    case 'profile':
      return { crumbs: [{ label: 'Profile' }] }
    case 'settings': {
      const crumbs: Crumb[] = [{ label: 'Settings', to: '/settings' }]
      if (id === 'members') crumbs.push({ label: 'Members' })
      else if (id === 'webhooks') crumbs.push({ label: 'Webhooks' })
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

  return (
    <header className="topbar">
      <button className="icon-button topbar-menu-button" onClick={onOpenDrawer} aria-label="Menu">
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
          <SearchNormal size={18} />
        </button>
        <button className="icon-button" onClick={toggleTheme} aria-label="Toggle theme">
          {theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}
        </button>
        <Dropdown
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
