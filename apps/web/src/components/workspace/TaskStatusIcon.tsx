import type { TaskStatus } from '../../mock/types'

const STATUS_COLOR: Record<TaskStatus, string> = {
  todo: 'var(--text-faint)',
  in_progress: 'var(--warning-dot)',
  done: 'var(--success-dot)',
  cancelled: 'var(--text-faint)',
}

/** Compact Linear-style status glyph: ○ ◐ ✓ ⊘ */
export function TaskStatusIcon({ status, size = 14 }: { status: TaskStatus; size?: number }) {
  const color = STATUS_COLOR[status]
  const common = {
    width: size,
    height: size,
    viewBox: '0 0 14 14',
    fill: 'none',
    style: { flexShrink: 0 },
  }
  switch (status) {
    case 'todo':
      return (
        <svg {...common} aria-label="Todo">
          <circle cx="7" cy="7" r="5.5" stroke={color} strokeWidth="1.5" />
        </svg>
      )
    case 'in_progress':
      return (
        <svg {...common} aria-label="In Progress">
          <circle cx="7" cy="7" r="5.5" stroke={color} strokeWidth="1.5" />
          <path d="M7 3.5 A3.5 3.5 0 0 1 7 10.5 Z" fill={color} />
        </svg>
      )
    case 'done':
      return (
        <svg {...common} aria-label="Done">
          <circle cx="7" cy="7" r="6" fill={color} />
          <path d="M4.4 7.2 L6.2 9 L9.6 5.4" stroke="var(--panel)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )
    case 'cancelled':
      return (
        <svg {...common} aria-label="Cancelled">
          <circle cx="7" cy="7" r="5.5" stroke={color} strokeWidth="1.5" />
          <path d="M4.8 4.8 L9.2 9.2" stroke={color} strokeWidth="1.5" strokeLinecap="round" />
        </svg>
      )
  }
}
