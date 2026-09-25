/**
 * Popover entrance for the views UI (spec § Motion): 150 ms strong ease-out from scale(0.97) + opacity,
 * scaling from the trigger. `data-instant` (keyboard-opened) turns it off. The `!` wins over the
 * primitive's zoom-in-95 / slide-in-from-top-2, which tailwind-merge cannot dedupe.
 */
export const POPOVER_MOTION =
  'duration-150 ease-[cubic-bezier(0.23,1,0.32,1)] data-open:zoom-in-[0.97]! data-closed:zoom-out-[0.97]! data-[side=bottom]:slide-in-from-top-0! data-[instant]:animate-none!'
