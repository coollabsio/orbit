import { Lock } from 'reicon-react'
import { cn } from 'cn'
import type { Project } from '@/features/tasks/api/models'

/** The lock after a private project's name. Nothing for a project the whole workspace sees. */
export function ProjectLock({ project, className }: { project: Pick<Project, 'private'>; className?: string }) {
  if (!project.private) return null
  return (
    <>
      <Lock aria-hidden="true" className={cn('size-3 shrink-0 text-muted-foreground/70', className)} />
      <span className="sr-only">Private</span>
    </>
  )
}
