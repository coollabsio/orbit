import { createContext } from 'react'
import type { CustomEmoji } from './customEmoji'

/** A workspace without custom emoji, and the list before it loaded. */
export const NO_CUSTOM_EMOJI: ReadonlyMap<string, CustomEmoji> = new Map()

/**
 * The custom emoji of the workspace by name, for everything that shows or offers one: message text, reactions, the
 * `:name` list and the picker. `ChatProvider` supplies it around the whole app shell, so `:name:` shows in task comments
 * too: the emoji belong to the workspace. Outside the shell it is empty and `:name:` stays text.
 */
export const CustomEmojiContext = createContext<ReadonlyMap<string, CustomEmoji>>(NO_CUSTOM_EMOJI)
