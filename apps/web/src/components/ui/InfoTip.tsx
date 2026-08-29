import { InfoCircle } from 'reicon-react'

/** Small info icon with a custom hover/focus tooltip (no native `title`). */
export function InfoTip({ text, size = 14 }: { text: string; size?: number }) {
  return (
    <span className="info-tip" tabIndex={0} role="img" aria-label={text} data-tip={text}>
      <InfoCircle size={size} />
    </span>
  )
}
