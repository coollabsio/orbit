import { Button } from '@/components/ui/button'
import { ZOOM_PRESETS, presetOf, type ZoomPreset } from './timelineLib'

const LABEL: Record<ZoomPreset, string> = { week: 'Week', month: 'Month', quarter: 'Quarter' }

/** Header controls shown only in the timeline layout. */
export function TimelineControls({ pxPerDay, onZoomChange, onToday }: { pxPerDay: number; onZoomChange: (px: number) => void; onToday: () => void }) {
  const active = presetOf(pxPerDay)
  return (
    <>
      <div role="group" aria-label="Timeline zoom" className="flex h-8 items-center rounded-lg border border-input bg-muted p-0.5 max-[899px]:hidden">
        {(Object.keys(ZOOM_PRESETS) as ZoomPreset[]).map((preset) => (
          <button
            key={preset}
            type="button"
            aria-pressed={preset === active}
            className="h-full rounded-md px-2 text-xs text-muted-foreground transition-[color,background-color,transform] duration-150 ease-out hover:text-foreground active:scale-[0.97] aria-pressed:bg-background aria-pressed:text-foreground aria-pressed:shadow-sm"
            onClick={() => onZoomChange(ZOOM_PRESETS[preset])}
          >
            {LABEL[preset]}
          </button>
        ))}
      </div>
      <Button variant="ghost" onClick={onToday}>Today</Button>
    </>
  )
}
