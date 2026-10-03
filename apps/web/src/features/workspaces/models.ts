export interface MemberAbilities {
  changeRole: boolean
  remove: boolean
  transferOwnership: boolean
}

/** For members built outside the members list (mocks, fixtures): no ability. */
export const NO_MEMBER_ABILITIES: MemberAbilities = { changeRole: false, remove: false, transferOwnership: false }

/** A workspace member. Shared by every feature that shows people. */
export interface User {
  id: string
  membershipId: string
  name: string
  handle: string
  email: string
  role: 'Owner' | 'Admin' | 'Member'
  color: string
  /** Profile picture URL; initials on `color` show while it is absent or loading. */
  avatarUrl?: string | null
  title: string
  roleIds: string[]
  version: number
  /** What the current user may do to this member. Decided by the server. */
  can: MemberAbilities
  /** The account is suspended: it cannot sign in, be mentioned or be notified. */
  suspended?: boolean
}
