import { Link } from 'react-router'
import { ArrowUpRight, Link2 } from 'reicon-react'
import type { GithubLink } from '@orbit/api/generated/types.gen'
import { parseGithubPullUrl, pullPath } from '@orbit/api/github'

const STATE_LABEL: Record<string, string> = { paused: 'Paused PR', merged: 'Merged PR', closed: 'Closed PR' }
const row = 'flex min-w-0 flex-1 items-center gap-2 rounded-md px-2 py-1.5 text-xs text-foreground transition-colors duration-150 ease-out hover:bg-muted'

/** The pull requests linked to a task: each opens in Orbit's viewer, with "Open on GitHub" next to it. */
export function TaskPullRequests({ links }: { links: GithubLink[] }) {
  return <div className="grid gap-1">
    {links.map((link) => {
      const ref = parseGithubPullUrl(link.url)
      const content = <>
        <Link2 className="size-4 shrink-0 text-muted-foreground" />
        <span className="truncate">{link.title}</span>
        <span className="ml-auto shrink-0 text-muted-foreground">{STATE_LABEL[link.state] ?? 'Open PR'}</span>
      </>
      return <div key={link.url} className="group/pull flex items-center gap-1">
        {ref ? <Link to={pullPath(ref)} className={row}>{content}</Link> : <a href={link.url} target="_blank" rel="noopener noreferrer" className={row}>{content}</a>}
        {ref ? <a href={link.url} target="_blank" rel="noopener noreferrer" aria-label="Open on GitHub" className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground opacity-0 transition-opacity duration-150 group-hover/pull:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100 pointer-coarse:opacity-100">
          <ArrowUpRight className="size-3.5" />
        </a> : null}
      </div>
    })}
  </div>
}
