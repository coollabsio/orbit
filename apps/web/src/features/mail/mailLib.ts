import type { LucideIcon } from 'lucide-react'
import { Archive, Folder, Inbox, Send, Star, StickyNote, Trash2 } from 'lucide-react'
import type { MailFolder, MailThread } from '../../mock/types'

export const DEFAULT_FOLDER_ID = 'f_inbox'
export const STARRED_FOLDER_ID = 'f_starred'

export const FOLDER_ICONS: Record<MailFolder['icon'], LucideIcon> = {
  inbox: Inbox,
  send: Send,
  note: StickyNote,
  trash: Trash2,
  archive: Archive,
  star: Star,
  folder: Folder,
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
