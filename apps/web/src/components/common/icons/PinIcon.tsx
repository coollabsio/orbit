import type { SVGProps } from 'react'

/** Pushpin glyph (24×24, two-tone mark). */
export function PinIcon({ size = 18, ...props }: { size?: number } & SVGProps<SVGSVGElement>) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true" {...props}>
      <g transform="rotate(45 12 12)">
        <path
          d="M5.75 3.25H18.25A1.5 1.5 0 0 1 19.75 4.75C19.75 9 16.75 10.5 16.75 13C16.75 15.5 19.1 15.75 19.1 18A1.25 1.25 0 0 1 17.85 19.25H6.15A1.25 1.25 0 0 1 4.9 18C4.9 15.75 7.25 15.5 7.25 13C7.25 10.5 4.25 9 4.25 4.75A1.5 1.5 0 0 1 5.75 3.25Z"
          fill="currentColor"
          fillOpacity="0.45"
        />
        <path d="M12 19.5V26" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      </g>
    </svg>
  )
}
