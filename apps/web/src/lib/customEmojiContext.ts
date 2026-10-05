import { createContext } from 'react'
import type { CustomEmoji } from './customEmoji'

/** A workspace without custom emoji, and the list before it loaded. */
export const NO_CUSTOM_EMOJI: ReadonlyMap<string, CustomEmoji> = new Map()

/**
 * The custom emoji of the workspace by name, for everything that shows or offers one: message text, reactions, the
 * `:name` list and the picker. Chat supplies it; anywhere else it is empty, so `:name:` stays text.
 */
export const CustomEmojiContext = createContext<ReadonlyMap<string, CustomEmoji>>(NO_CUSTOM_EMOJI)
