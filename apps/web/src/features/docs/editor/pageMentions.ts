// @mentions in the page body: who the "@" picker offers and how a chip is labelled. Kept free of BlockNote imports
// (DocEditor in the main chunk uses it; the editor chunk loads lazily).
import { createContext } from 'react'

/** A member the picker can offer (a workspace member as `useMembers` returns them). */
export interface PageMentionMember {
  id: string
  name: string
  /** Secondary text in the picker (the email handle). */
  handle?: string
  email?: string
  color?: string
  /** Suspended members cannot see anything and are never offered. */
  suspended?: boolean
}

/** What the page body's "@" picker offers, and whether to say that the page is private. */
export interface PageMentionOptions {
  /** Workspace members (live names for chips). `null` while loading: chips show their stored name. */
  members: readonly PageMentionMember[] | null
  /** Who can be mentioned: the members who can see the page. */
  candidates: readonly PageMentionMember[]
  /** A private page: only its owner can see it, so only they are offered. */
  privatePage: boolean
}

export const PRIVATE_PAGE_HINT = 'Only you can see this private page'
export const UNKNOWN_USER = 'Unknown user'

/**
 * The members who can see the page: every active member for a teamspace page, only the owner (the signed-in user)
 * for a private page. The server applies the same rule before it notifies anyone.
 */
export function pageMentionCandidates<T extends PageMentionMember>(
  members: readonly T[],
  page: { teamspace_id: string | null },
  selfId: string | null,
): T[] {
  const active = members.filter((member) => !member.suspended)
  if (page.teamspace_id === null) return active.filter((member) => member.id === selfId)
  // Yourself last: you rarely mention yourself.
  return [...active.filter((member) => member.id !== selfId), ...active.filter((member) => member.id === selfId)]
}

/** Picker entries for `query` (name, handle or email, case-insensitive), at most 10. */
export function filterPageMentionCandidates<T extends PageMentionMember>(candidates: readonly T[], query: string): T[] {
  const needle = query.trim().toLowerCase()
  return candidates
    .filter(
      (candidate) =>
        !needle ||
        candidate.name.toLowerCase().includes(needle) ||
        candidate.handle?.toLowerCase().includes(needle) ||
        candidate.email?.toLowerCase().includes(needle),
    )
    .slice(0, 10)
}

/**
 * A chip's label without the "@": the member's current name; the stored name while the member list is loading (or
 * in read-only previews without one); "Unknown user" once the member is gone.
 */
export function mentionLabel(userId: string, storedName: string, members: ReadonlyMap<string, string> | null): string {
  if (members) return members.get(userId) ?? UNKNOWN_USER
  return storedName.trim() || UNKNOWN_USER
}

/** Current member names by user id; `null` while unknown (chips then show their stored name). */
export const PageMentionNamesContext = createContext<ReadonlyMap<string, string> | null>(null)
