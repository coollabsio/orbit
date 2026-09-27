import { createContext } from 'react'
import type { MentionCandidate } from './mentionable'

/** Current display names by user id (mentions store the name at the time, this keeps renames fresh). */
export const MentionNamesContext = createContext<ReadonlyMap<string, string>>(new Map())

/** Who the "@" picker offers in comment editors. */
export const MentionCandidatesContext = createContext<readonly MentionCandidate[]>([])

/** Workspace members by id: comment authors (name + avatar color) in thread cards. */
export const CommentMembersContext = createContext<ReadonlyMap<string, MentionCandidate>>(new Map())
