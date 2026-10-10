import { useState, type ReactNode } from 'react'
import { cn } from 'cn'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { niceMax, type Measure } from './chartLib'

/** The three series colours (index.css `--viz-*`), in fixed order. Text never takes a series colour. */
export const SERIES = { done: 'var(--viz-1)', started: 'var(--viz-2)', scope: 'var(--viz-3)', neutral: 'var(--muted-foreground)' } as const

export interface ChartSeries {
  name: string
  color: string
  values: number[]
  /** A reference line (ideal, average): thin and dashed, with no markers. */
  dashed?: boolean
}

const WIDTH = 640
const HEIGHT = 200
const PAD = { top: 12, right: 12, bottom: 22, left: 34 }
const PLOT_WIDTH = WIDTH - PAD.left - PAD.right
const PLOT_HEIGHT = HEIGHT - PAD.top - PAD.bottom

/** A titled chart with a legend, a one-line note and a text summary for screen readers. */
export function ChartCard({ title, summary, legend, note, actions, children }: {
  title: string
  /** What the chart shows, in words: read by screen readers in place of the drawing. */
  summary: string
  legend?: Array<Pick<ChartSeries, 'name' | 'color' | 'dashed'>>
  note?: ReactNode
  actions?: ReactNode
  children: ReactNode
}) {
  return (
    <section aria-label={title} className="flex flex-col gap-2 rounded-lg border p-4">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <h3 className="text-sm font-medium text-foreground">{title}</h3>
        {legend && legend.length > 1 ? (
          <ul aria-hidden className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
            {legend.map((item) => (
              <li key={item.name} className="flex items-center gap-1.5">
                <span className={cn('h-0.5 w-3 rounded-full', item.dashed && 'opacity-60')} style={{ background: item.color }} />
                {item.name}
              </li>
            ))}
          </ul>
        ) : null}
        {actions ? <div className="ml-auto">{actions}</div> : null}
      </div>
      <p className="sr-only">{summary}</p>
      {children}
      {note ? <p className="text-xs text-muted-foreground">{note}</p> : null}
    </section>
  )
}

/** One line of text in place of a chart that has no data yet. */
export function ChartEmpty({ children }: { children: ReactNode }) {
  return <p className="py-6 text-center text-[13px] text-muted-foreground">{children}</p>
}

/** Tasks or points. Shown only where the project has estimates on. */
export function MeasureToggle({ measure, onChange }: { measure: Measure; onChange: (measure: Measure) => void }) {
  return (
    <ToggleGroup
      aria-label="Measure"
      variant="outline"
      size="sm"
      spacing={0}
      value={[measure]}
      onValueChange={(value: string[]) => {
        // pressing the active item again empties the group: keep the measure
        const next = value[0]
        if (next === 'count' || next === 'points') onChange(next)
      }}
    >
      <ToggleGroupItem value="count">Tasks</ToggleGroupItem>
      <ToggleGroupItem value="points">Points</ToggleGroupItem>
    </ToggleGroup>
  )
}

function Axes({ max, labels, every }: { max: number; labels: string[]; every: number }) {
  const step = labels.length > 1 ? PLOT_WIDTH / (labels.length - 1) : 0
  return (
    <g className="text-[10px]" fill="var(--muted-foreground)">
      {[0, 0.5, 1].map((share) => {
        const y = PAD.top + PLOT_HEIGHT * (1 - share)
        return (
          <g key={share}>
            <line x1={PAD.left} x2={WIDTH - PAD.right} y1={y} y2={y} stroke="var(--border)" strokeWidth="1" />
            <text x={PAD.left - 6} y={y + 3} textAnchor="end">{Math.round(max * share * 10) / 10}</text>
          </g>
        )
      })}
      {labels.map((label, index) => (index % every === 0 || index === labels.length - 1 ? (
        // eslint-disable-next-line react/no-array-index-key -- positions on the axis; a label can repeat
        <text key={index} x={PAD.left + (labels.length > 1 ? index * step : PLOT_WIDTH / 2)} y={HEIGHT - 6} textAnchor={index === 0 ? 'start' : index === labels.length - 1 ? 'end' : 'middle'}>{label}</text>
      ) : null))}
    </g>
  )
}

function Tooltip({ x, title, rows }: { x: number; title: string; rows: Array<{ name: string; color: string; value: string }> }) {
  return (
    <div
      role="status"
      className="pointer-events-none absolute top-1 z-10 min-w-32 -translate-x-1/2 rounded-md bg-popover px-2 py-1.5 text-xs text-popover-foreground shadow-md ring-1 ring-foreground/10"
      style={{ left: `${Math.min(88, Math.max(12, (x / WIDTH) * 100))}%` }}
    >
      <div className="mb-1 font-medium">{title}</div>
      {rows.map((row) => (
        <div key={row.name} className="flex items-center gap-1.5">
          <span className="size-2 rounded-full" style={{ background: row.color }} />
          <span className="flex-1 text-muted-foreground">{row.name}</span>
          <span className="tabular-nums">{row.value}</span>
        </div>
      ))}
    </div>
  )
}

/**
 * Lines over a shared axis of days or weeks. Hover (or focus and the arrow keys) shows a crosshair and the exact
 * values of that position. `marker` draws a vertical line at a position (a target date).
 */
export function LineChart({ series, labels, tooltipLabels = labels, marker, format = String }: {
  series: ChartSeries[]
  /** Axis labels, one for each position. */
  labels: string[]
  tooltipLabels?: string[]
  marker?: { index: number; label: string }
  format?: (value: number) => string
}) {
  const [active, setActive] = useState<number | null>(null)
  const count = labels.length
  const max = niceMax(Math.max(0, ...series.flatMap((item) => item.values)))
  const x = (index: number) => PAD.left + (count > 1 ? (index * PLOT_WIDTH) / (count - 1) : PLOT_WIDTH / 2)
  const y = (value: number) => PAD.top + PLOT_HEIGHT * (1 - value / max)
  const indexAt = (clientX: number, rect: DOMRect) =>
    Math.max(0, Math.min(count - 1, Math.round((((clientX - rect.left) / rect.width) * WIDTH - PAD.left) / (count > 1 ? PLOT_WIDTH / (count - 1) : 1))))
  return (
    <div className="relative">
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        role="img"
        aria-label="Chart. Use the left and right arrow keys to read each position."
        tabIndex={0}
        className="block h-auto w-full outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        onPointerMove={(event) => setActive(indexAt(event.clientX, event.currentTarget.getBoundingClientRect()))}
        onPointerLeave={() => setActive(null)}
        onBlur={() => setActive(null)}
        onKeyDown={(event) => {
          if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
          event.preventDefault()
          setActive((current) => Math.max(0, Math.min(count - 1, (current ?? (event.key === 'ArrowRight' ? -1 : count)) + (event.key === 'ArrowRight' ? 1 : -1))))
        }}
      >
        <Axes max={max} labels={labels} every={Math.max(1, Math.ceil(count / 8))} />
        {marker && marker.index >= 0 && marker.index < count ? (
          <g>
            <line x1={x(marker.index)} x2={x(marker.index)} y1={PAD.top} y2={PAD.top + PLOT_HEIGHT} stroke="var(--foreground)" strokeWidth="1" strokeDasharray="2 3" opacity="0.5" />
            <text x={x(marker.index) + (marker.index > count / 2 ? -4 : 4)} y={PAD.top + 9} textAnchor={marker.index > count / 2 ? 'end' : 'start'} className="text-[10px]" fill="var(--muted-foreground)">{marker.label}</text>
          </g>
        ) : null}
        {series.map((item) => (
          <g key={item.name}>
            <polyline
              fill="none"
              stroke={item.color}
              strokeWidth={item.dashed ? 1.5 : 2}
              strokeDasharray={item.dashed ? '4 4' : undefined}
              strokeLinejoin="round"
              strokeLinecap="round"
              opacity={item.dashed ? 0.7 : 1}
              points={item.values.map((value, index) => `${x(index)},${y(value)}`).join(' ')}
            />
            {/* a single position has no line to draw: show its point */}
            {!item.dashed && count === 1 ? <circle cx={x(0)} cy={y(item.values[0] ?? 0)} r="4" fill={item.color} stroke="var(--background)" strokeWidth="2" /> : null}
          </g>
        ))}
        {active !== null ? (
          <g>
            <line x1={x(active)} x2={x(active)} y1={PAD.top} y2={PAD.top + PLOT_HEIGHT} stroke="var(--muted-foreground)" strokeWidth="1" opacity="0.6" />
            {series.filter((item) => !item.dashed).map((item) => (
              <circle key={item.name} cx={x(active)} cy={y(item.values[active] ?? 0)} r="4" fill={item.color} stroke="var(--background)" strokeWidth="2" />
            ))}
          </g>
        ) : null}
      </svg>
      {active !== null ? (
        <Tooltip x={x(active)} title={tooltipLabels[active] ?? ''} rows={series.map((item) => ({ name: item.name, color: item.color, value: format(item.values[active] ?? 0) }))} />
      ) : null}
    </div>
  )
}

/** Bars for each position, side by side for two series, with an optional reference line (an average). */
export function BarChart({ series, labels, tooltipLabels = labels, reference, format = String }: {
  series: ChartSeries[]
  labels: string[]
  tooltipLabels?: string[]
  reference?: { value: number; label: string }
  format?: (value: number) => string
}) {
  const [active, setActive] = useState<number | null>(null)
  const count = labels.length
  const max = niceMax(Math.max(reference?.value ?? 0, ...series.flatMap((item) => item.values)))
  const band = PLOT_WIDTH / Math.max(1, count)
  // thin bars with a 2px gap between two bars of one position
  const bar = Math.min(18, (band - 8) / series.length - 2)
  const y = (value: number) => PAD.top + PLOT_HEIGHT * (1 - value / max)
  const center = (index: number) => PAD.left + band * (index + 0.5)
  return (
    <div className="relative">
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        role="img"
        aria-label="Chart. Use the left and right arrow keys to read each position."
        tabIndex={0}
        className="block h-auto w-full outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        onPointerMove={(event) => {
          const rect = event.currentTarget.getBoundingClientRect()
          setActive(Math.max(0, Math.min(count - 1, Math.floor((((event.clientX - rect.left) / rect.width) * WIDTH - PAD.left) / band))))
        }}
        onPointerLeave={() => setActive(null)}
        onBlur={() => setActive(null)}
        onKeyDown={(event) => {
          if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
          event.preventDefault()
          setActive((current) => Math.max(0, Math.min(count - 1, (current ?? (event.key === 'ArrowRight' ? -1 : count)) + (event.key === 'ArrowRight' ? 1 : -1))))
        }}
      >
        <Axes max={max} labels={[]} every={1} />
        {active !== null ? <rect x={PAD.left + band * active} y={PAD.top} width={band} height={PLOT_HEIGHT} fill="var(--foreground)" opacity="0.04" /> : null}
        {labels.map((label, index) => (
          // eslint-disable-next-line react/no-array-index-key -- positions on the axis; a label can repeat
          <g key={index}>
            {series.map((item, at) => {
              const value = item.values[index] ?? 0
              const top = y(value)
              const left = center(index) - (series.length * (bar + 2) - 2) / 2 + at * (bar + 2)
              const height = PAD.top + PLOT_HEIGHT - top
              const radius = Math.min(4, height, bar / 2)
              // rounded at the data end, square on the baseline
              return height > 0 ? (
                <path key={item.name} fill={item.color} d={`M${left},${top + height} V${top + radius} Q${left},${top} ${left + radius},${top} H${left + bar - radius} Q${left + bar},${top} ${left + bar},${top + radius} V${top + height} Z`} />
              ) : null
            })}
            {index % Math.max(1, Math.ceil(count / 8)) === 0 || index === count - 1 ? (
              <text x={center(index)} y={HEIGHT - 6} textAnchor="middle" className="text-[10px]" fill="var(--muted-foreground)">{label}</text>
            ) : null}
          </g>
        ))}
        {reference ? (
          <g>
            <line x1={PAD.left} x2={WIDTH - PAD.right} y1={y(reference.value)} y2={y(reference.value)} stroke="var(--foreground)" strokeWidth="1.5" strokeDasharray="4 4" opacity="0.6" />
            <text x={WIDTH - PAD.right} y={y(reference.value) - 4} textAnchor="end" className="text-[10px]" fill="var(--muted-foreground)">{reference.label}</text>
          </g>
        ) : null}
      </svg>
      {active !== null ? (
        <Tooltip x={center(active)} title={tooltipLabels[active] ?? ''} rows={series.map((item) => ({ name: item.name, color: item.color, value: format(item.values[active] ?? 0) }))} />
      ) : null}
    </div>
  )
}

/** Horizontal bars with the name and the exact value in text on each row: a ranking of groups. */
export function BarList({ rows, format = String }: { rows: Array<{ key: string; label: string; value: number; color?: string }>; format?: (value: number) => string }) {
  const max = Math.max(1, ...rows.map((row) => row.value))
  return (
    <ul className="flex flex-col gap-1.5">
      {rows.map((row) => (
        <li key={row.key} className="grid grid-cols-[minmax(0,9rem)_1fr_auto] items-center gap-2 text-xs" title={`${row.label}: ${format(row.value)}`}>
          <span className="truncate text-muted-foreground">{row.label}</span>
          <span className="h-2 rounded-r bg-muted">
            <span className="block h-2 rounded-r" style={{ width: `${(row.value / max) * 100}%`, background: row.color ?? SERIES.done }} />
          </span>
          <span className="text-foreground tabular-nums">{format(row.value)}</span>
        </li>
      ))}
    </ul>
  )
}
