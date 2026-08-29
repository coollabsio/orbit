import type { SVGProps } from 'react'

/** Thread glyph (24×24, three-tone mark). */
export function ThreadIcon({ size = 18, ...props }: { size?: number } & SVGProps<SVGSVGElement>) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" {...props}>
      <rect x="1.25" y="1.25" width="9.5" height="21.5" rx="4.75" fillOpacity="0.35" />
      <path
        d="M10.75 6.432L12.7833 4.39868A4.75 4.75 0 0 1 19.5008 11.1162L7.867 22.75H6A4.75 4.75 0 0 0 10.75 18Z"
        fillOpacity="0.66"
      />
      <path d="M17.367 13.25H18A4.75 4.75 0 0 1 18 22.75H7.867Z" />
      <circle cx="6" cy="18" r="1.1" />
    </svg>
  )
}
