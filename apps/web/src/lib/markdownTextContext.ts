import { createContext, type ReactNode } from 'react'

/**
 * Lets a host draw parts of the plain text of rendered markdown its own way (chat: a task chip for `ENG-12`). It gets a
 * run of text that has no markdown, link or mention in it, and `plain`, which renders text the default way, for the
 * parts it leaves alone.
 */
export const MarkdownTextContext = createContext<((text: string, plain: (text: string) => ReactNode) => ReactNode) | null>(null)
