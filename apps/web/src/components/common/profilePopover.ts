import { createContext } from 'react'

/**
 * What a `ProfileTrigger` needs of the one profile popover of the app. The shell provides it (`app/shell/ProfilePopover`),
 * because the popover reads features and nothing under `components/` may.
 */
export interface ProfilePopoverControl {
  /**
   * Shows the profile of `userId` beside `anchor`. While the popover shows somebody else it moves there; a second
   * call with the anchor it is open on closes it. `instant`: opened from the keyboard, so without the entrance motion.
   */
  open: (userId: string, anchor: HTMLElement, options: { name: string; instant: boolean }) => void
  /** Loads what the popover shows, so that it opens with its content. */
  prefetch: (userId: string) => void
}

/** `null` outside the signed-in shell: a trigger is plain content there. */
export const ProfilePopoverContext = createContext<ProfilePopoverControl | null>(null)

/** Marks a trigger in the DOM (its value is the kind): a press on one while the popover is open moves the popover. */
export const PROFILE_TRIGGER_ATTRIBUTE = 'data-profile-trigger'
