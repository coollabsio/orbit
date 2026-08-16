import type { IconComponent } from 'reicon-react'
import { Archive, DirectInbox, Note2, Send2, Star, Trash } from 'reicon-react'
import type { MailFolder, MailThread } from '../../mock/types'

export const DEFAULT_FOLDER_ID = 'f_inbox'
export const STARRED_FOLDER_ID = 'f_starred'

export const FOLDER_ICONS: Record<MailFolder['icon'], IconComponent> = {
  inbox: DirectInbox,
  send: Send2,
  note: Note2,
  trash: Trash,
  archive: Archive,
  star: Star,
}

/** Threads belonging to a folder, newest first. "Starred" is virtual. */
export function threadsInFolder(threads: MailThread[], folderId: string): MailThread[] {
  const filtered =
    folderId === STARRED_FOLDER_ID
      ? threads.filter((t) => t.starred)
      : threads.filter((t) => t.folderId === folderId)
  return [...filtered].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
}

export function folderUnreadCount(threads: MailThread[], folderId: string): number {
  return threadsInFolder(threads, folderId).filter((t) => t.unread).length
}

/** Sender of the most recent message in a thread. */
export function threadSender(thread: MailThread): { name: string; email: string } {
  return thread.messages[thread.messages.length - 1]?.from ?? { name: 'Unknown', email: '' }
}

/** First non-empty line of a message body, for collapsed previews. */
export function firstLine(body: string): string {
  return body.split('\n').find((line) => line.trim() !== '') ?? ''
}
