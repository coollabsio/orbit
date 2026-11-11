/**
 * Projects in the member's own sidebar order (`GET /project-order`): the placed ones first, in that order, then the
 * others in the order they came (the API lists projects by name).
 */
export function orderProjects<T extends { id: string }>(projects: readonly T[], order: readonly string[]): T[] {
  const rank = new Map(order.map((id, index) => [id, index]))
  return projects
    .map((project, index) => ({ project, rank: rank.get(project.id) ?? order.length + index }))
    .sort((a, b) => a.rank - b.rank)
    .map((entry) => entry.project)
}

/** The ids after moving `id` to index `to`; `null` when that changes nothing or is out of range. */
export function moveProject(ids: readonly string[], id: string, to: number): string[] | null {
  const from = ids.indexOf(id)
  if (from < 0 || to < 0 || to >= ids.length || from === to) return null
  const next = ids.filter((other) => other !== id)
  next.splice(to, 0, id)
  return next
}
