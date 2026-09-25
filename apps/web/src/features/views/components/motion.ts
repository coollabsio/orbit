/**
 * Popover entrance for the views UI (spec § Motion): 150 ms strong ease-out from scale(0.97) + opacity,
 * scaling from the trigger. `data-instant` (keyboard-opened) turns it off. The `!` wins over the
 * primitive's zoom-in-95 / slide-in-from-top-2, which tailwind-merge cannot dedupe.
 */
export const POPOVER_MOTION =
  'duration-150 ease-[cubic-bezier(0.23,1,0.32,1)] data-open:zoom-in-[0.97]! data-closed:zoom-out-[0.97]! data-[side=bottom]:slide-in-from-top-0! data-[instant]:animate-none!'

/**
 * Modal entrance (spec § Motion): 200 ms fade + scale(0.95 → 1) on the strong ease-out, from the centre as a modal
 * should. Reduced motion keeps the fade only; `data-instant` (opened from the keyboard) turns it off. Closing is
 * instant: the content unmounts with `open`, like the views popovers.
 */
export const DIALOG_MOTION =
  'origin-center duration-200 ease-[cubic-bezier(0.23,1,0.32,1)] motion-reduce:data-open:zoom-in-100 data-[instant]:animate-none'

/** Press feedback for buttons: scale(0.97) on :active instead of the primitive's 1px drop; none under reduced motion. */
export const PRESS_MOTION =
  'transition-[color,background-color,scale] duration-150 ease-out active:not-aria-[haspopup]:translate-y-0 active:scale-[0.97] motion-reduce:active:scale-100'
