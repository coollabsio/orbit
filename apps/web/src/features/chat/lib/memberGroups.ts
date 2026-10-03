interface Member {
  id: string
  name: string
  role: 'Owner' | 'Admin' | 'Member'
}

export interface MemberGroup<T> {
  label: string
  online: boolean
  people: T[]
}

/**
 * The member list in Discord's order: people who are online under their role (Owner, Admin, then everybody else as
 * "Online"), and everybody who is offline together at the end. Names sort inside a group; empty groups are left out.
 */
export function memberGroups<T extends Member>(people: readonly T[], online: ReadonlySet<string>): MemberGroup<T>[] {
  const sorted = [...people].sort((a, b) => a.name.localeCompare(b.name))
  const here = sorted.filter((person) => online.has(person.id))
  return [
    { label: 'Owner', online: true, people: here.filter((person) => person.role === 'Owner') },
    { label: 'Admin', online: true, people: here.filter((person) => person.role === 'Admin') },
    { label: 'Online', online: true, people: here.filter((person) => person.role === 'Member') },
    { label: 'Offline', online: false, people: sorted.filter((person) => !online.has(person.id)) },
  ].filter((group) => group.people.length > 0)
}
