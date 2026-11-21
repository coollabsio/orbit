import { cn } from 'cn'

import { ColorDot } from './ColorDot'

/**
 * A project's mark: its icon image when it has one, else its colour dot. `className` styles the dot,
 * `iconClassName` sizes the image (an icon needs more room than a dot to be readable).
 */
export function ProjectMark({
  project,
  className,
  iconClassName = 'size-4',
}: {
  project: { color: string; icon_url?: string | null } | undefined
  className?: string
  iconClassName?: string
}) {
  if (project?.icon_url) {
    return <img src={project.icon_url} alt="" aria-hidden data-slot="project-icon" draggable={false} className={cn('shrink-0 rounded-[4px] object-cover', iconClassName)} />
  }
  return <ColorDot color={project?.color} className={className} />
}
