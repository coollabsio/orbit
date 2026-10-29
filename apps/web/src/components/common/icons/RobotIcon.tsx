import { createIcon } from 'reicon-react'

/** reicon has no robot, so this one copies its Outline style (24×24, 1.5 stroke, round caps). Mobile: `brand-icons.ts`. */
export const RobotIcon = createIcon('Robot', {
  O: '<g stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="7" width="16" height="12" rx="3.5"/><path d="M12 4v3M10 15.5h4M2 11.5v3M22 11.5v3"/><circle cx="12" cy="3.25" r="0.75"/></g><circle cx="9" cy="12" r="1" fill="currentColor"/><circle cx="15" cy="12" r="1" fill="currentColor"/>',
})
