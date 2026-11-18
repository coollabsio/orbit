import { useMemo } from 'react'
import type { MailPerson } from '@orbit/api/generated/types.gen'
import type { AvatarPerson } from '@/components/common/UserAvatar'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { personName, senderColor } from '@/features/mail/mailLib'

/** The avatar of a mail person: the member with that account address (photo and colour), else a colour of the address. */
export type AvatarOf = (person: MailPerson | null | undefined, name?: string) => AvatarPerson

export function useMailAvatar(): AvatarOf {
  const members = useMembers(useWorkspace().workspace.id).data
  return useMemo(() => {
    const byEmail = new Map((members ?? []).map((member) => [member.email.toLowerCase(), member]))
    return (person, name) => {
      const member = person ? byEmail.get(person.address.toLowerCase()) : undefined
      if (member) return { name: member.name, color: member.color, avatarUrl: member.avatarUrl }
      return { name: name ?? personName(person), color: senderColor(person?.address ?? name) }
    }
  }, [members])
}
