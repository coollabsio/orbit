import type { TaskPriority } from '../../mock/types'
import { PRIORITY_LABEL } from './taskMeta'

/** Compact priority bars, Linear-style. Urgent renders a warning square. */
export function PriorityIcon({ priority, size = 14 }: { priority: TaskPriority; size?: number }) {
  const common = {
    width: size,
    height: size,
    viewBox: '0 0 14 14',
    fill: 'none',
    style: { flexShrink: 0 },
  }
  if (priority === 'urgent') {
    return (
      <svg {...common} aria-label="Urgent">
        <rect x="1" y="1" width="12" height="12" rx="3" fill="var(--danger-dot)" />
        <path d="M7 3.8 V8" stroke="#fff" strokeWidth="1.6" strokeLinecap="round" />
        <circle cx="7" cy="10.4" r="0.9" fill="#fff" />
      </svg>
    )
  }
  const active = 'var(--text-secondary)'
  const inactive = 'var(--fill)'
  const level = priority === 'high' ? 3 : priority === 'medium' ? 2 : priority === 'low' ? 1 : 0
  return (
    <svg {...common} aria-label={PRIORITY_LABEL[priority]}>
      <rect x="1.5" y="8" width="2.6" height="4.5" rx="1" fill={level >= 1 ? active : inactive} />
      <rect x="5.7" y="5.5" width="2.6" height="7" rx="1" fill={level >= 2 ? active : inactive} />
      <rect x="9.9" y="2.5" width="2.6" height="10" rx="1" fill={level >= 3 ? active : inactive} />
    </svg>
  )
}
