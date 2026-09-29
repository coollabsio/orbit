import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { ZOOM_PRESETS, presetOf, type ZoomPreset } from './timelineLib'

const LABEL: Record<ZoomPreset, string> = { week: 'Week', month: 'Month', quarter: 'Quarter' }

/** Header controls shown only in the timeline layout. */
export function TimelineControls({ pxPerDay, onZoomChange, onToday }: { pxPerDay: number; onZoomChange: (px: number) => void; onToday: () => void }) {
  const active = presetOf(pxPerDay)
  return (
    <>
      <ToggleGroup
        aria-label="Timeline zoom"
        variant="outline"
        size="sm"
        spacing={0}
        className="max-[899px]:hidden"
        value={active ? [active] : []}
        onValueChange={(value: string[]) => {
          // pressing the active preset again empties the group: keep the zoom
          const next = value[0] as ZoomPreset | undefined
          if (next) onZoomChange(ZOOM_PRESETS[next])
        }}
      >
        {(Object.keys(ZOOM_PRESETS) as ZoomPreset[]).map((preset) => (
          <ToggleGroupItem key={preset} value={preset}>
            {LABEL[preset]}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <Button variant="ghost" onClick={onToday}>Today</Button>
    </>
  )
}
