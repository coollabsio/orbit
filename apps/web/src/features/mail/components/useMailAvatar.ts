import { useMemo } from 'react'
import type { MailPerson } from '@orbit/api/generated/types.gen'
import type { AvatarPerson } from '@/components/common/UserAvatar'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { personName, senderColor } from '@/features/mail/mailLib'

/**
 * The avatar of a mail person: a colour of the address; with `trusted`, the member with that account address (photo
 * and colour). Trusted means Orbit sent the message or DMARC confirmed the sender's domain: a forged From must not
 * get a member's photo.
 */
export type AvatarOf = (person: MailPerson | null | undefined, name?: string, trusted?: boolean) => AvatarPerson

export function useMailAvatar(): AvatarOf {
  const members = useMembers(useWorkspace().workspace.id).data
  return useMemo(() => {
    const byEmail = new Map((members ?? []).map((member) => [member.email.toLowerCase(), member]))
    return (person, name, trusted = false) => {
      const member = trusted && person ? byEmail.get(person.address.toLowerCase()) : undefined
      if (member) return { name: member.name, color: member.color, avatarUrl: member.avatarUrl }
      return { name: name ?? personName(person), color: senderColor(person?.address ?? name) }
    }
  }, [members])
}
