/** A workspace member. Shared by every feature that shows people. */
export interface User {
  id: string
  membershipId: string
  name: string
  handle: string
  email: string
  role: 'Owner' | 'Admin' | 'Member'
  color: string
  online: boolean
  title: string
  roleIds: string[]
  version: number
}
