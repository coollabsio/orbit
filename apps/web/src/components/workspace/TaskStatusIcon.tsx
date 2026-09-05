import type { StatusCategory, TaskStatusDef } from '../../features/tasks/api/models'

/**
 * Compact Linear-style status glyph. The shape follows the category (○ ◐ ✓ ⊘), the color the status.
 * `status` may be a full definition or just `{ category, color }` (e.g. while editing).
 */
export function TaskStatusIcon({
  status,
  size = 14,
}: {
  status: Pick<TaskStatusDef, 'category' | 'color'> | undefined
  size?: number
}) {
  const common = {
    width: size,
    height: size,
    viewBox: '0 0 14 14',
    fill: 'none',
    style: { flexShrink: 0 },
  }
  if (!status) {
    return (
      <svg {...common} aria-label="No status">
        <circle cx="7" cy="7" r="5.5" stroke="var(--text-faint)" strokeWidth="1.5" strokeDasharray="2 2" />
      </svg>
    )
  }
  const color = status.color
  const category: StatusCategory = status.category
  switch (category) {
    case 'unstarted':
      return (
        <svg {...common} aria-label="Unstarted">
          <circle cx="7" cy="7" r="5.5" stroke={color} strokeWidth="1.5" />
        </svg>
      )
    case 'started':
      return (
        <svg {...common} aria-label="Started">
          <circle cx="7" cy="7" r="5.5" stroke={color} strokeWidth="1.5" />
          <path d="M7 3.5 A3.5 3.5 0 0 1 7 10.5 Z" fill={color} />
        </svg>
      )
    case 'completed':
      return (
        <svg {...common} aria-label="Completed">
          <circle cx="7" cy="7" r="6" fill={color} />
          <path d="M4.4 7.2 L6.2 9 L9.6 5.4" stroke="var(--panel)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )
    case 'cancelled':
      return (
        <svg {...common} aria-label="Cancelled">
          <circle cx="7" cy="7" r="6" fill={color} />
          <path d="M4.8 4.8 L9.2 9.2 M9.2 4.8 L4.8 9.2" stroke="var(--panel)" strokeWidth="1.5" strokeLinecap="round" />
        </svg>
      )
  }
}
