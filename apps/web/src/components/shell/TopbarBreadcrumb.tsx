import { Fragment } from 'react'
import { Link } from 'react-router'

export interface Crumb {
  label: string
  to?: string
  /** A placeholder rather than a place, e.g. a sub-issue's parent that is in the trash. */
  muted?: boolean
}

/**
 * Page-supplied breadcrumb segments. The shell already renders the workspace root crumb, so every
 * segment here is prefixed with a separator. Plan 04-task-graph adds a parent issue by passing one
 * more element in `crumbs`.
 */
export function TopbarBreadcrumb({ crumbs }: { crumbs: Crumb[] }) {
  return (
    <>
      {crumbs.map((crumb, index) => {
        const isLast = index === crumbs.length - 1
        return (
          <Fragment key={`${crumb.label}-${index}`}>
            <span className="topbar-crumb-sep" aria-hidden="true">›</span>
            {crumb.to && !isLast ? (
              <Link className="topbar-crumb" to={crumb.to}>{crumb.label}</Link>
            ) : (
              <span className="topbar-crumb" data-current={isLast} data-muted={crumb.muted || undefined}>{crumb.label}</span>
            )}
          </Fragment>
        )
      })}
    </>
  )
}
