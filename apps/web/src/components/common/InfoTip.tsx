import { InfoCircle as Info } from 'reicon-react'
import { HelpTip } from '@/components/common/HelpTip'

/** Small info icon that shows its text on hover, focus or tap. */
export function InfoTip({ text, size = 14 }: { text: string; size?: number }) {
  return (
    <HelpTip text={text}>
      <span
        className="inline-flex cursor-pointer text-muted-foreground"
        tabIndex={0}
        role="button"
        aria-label={text}
        // Inside a <label>, a click would focus or toggle the label's control.
        onClick={(event) => event.preventDefault()}
      >
        <Info size={size} aria-hidden="true" />
      </span>
    </HelpTip>
  )
}
