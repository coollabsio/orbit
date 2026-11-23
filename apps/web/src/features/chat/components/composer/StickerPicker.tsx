import { useState } from 'react'
import { Search4 } from 'reicon-react'
import type { CustomStickerRecord, KlipyItem } from '@orbit/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { searchStickers } from '@/lib/customSticker'
import { KlipyResults, PoweredByKlipy } from './KlipyResults'

/**
 * The sticker panel of the composer: a search field, the workspace's stickers four in a row, then (when an admin set a
 * KLIPY key, `klipyWorkspaceId`) stickers from KLIPY, trending or found with the same search. Each is a button, so
 * `Tab` goes through them; a click picks one.
 */
export function StickerPicker({
  stickers,
  klipyWorkspaceId,
  onPick,
  onPickKlipy,
}: {
  stickers: readonly CustomStickerRecord[]
  /** The workspace whose KLIPY stickers show; `null` shows only the workspace's own. */
  klipyWorkspaceId: string | null
  onPick: (sticker: CustomStickerRecord) => void
  onPickKlipy: (sticker: KlipyItem) => void
}) {
  const [search, setSearch] = useState('')
  const found = searchStickers(stickers, search)
  // KLIPY's rules: a field that searches KLIPY names it
  const placeholder = klipyWorkspaceId ? 'Search stickers and KLIPY' : 'Search stickers'
  return (
    <div data-slot="sticker-picker" className="flex max-h-[380px] w-[344px] max-w-[calc(100vw-2rem)] flex-col">
      <div className="p-2">
        <InputGroup className="border-0 bg-secondary/40 dark:bg-secondary/40">
          <InputGroupAddon>
            <Search4 />
          </InputGroupAddon>
          <InputGroupInput autoFocus type="search" value={search} placeholder={placeholder} aria-label={placeholder} autoComplete="off" onChange={(event) => setSearch(event.target.value)} />
        </InputGroup>
      </div>
      <div data-slot="klipy-scroll" className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-2">
        {found.length > 0 ? (
          <section aria-label={klipyWorkspaceId ? 'Stickers of this workspace' : undefined}>
            {klipyWorkspaceId ? <h3 className="px-1 pb-1 text-xs font-medium text-muted-foreground">This workspace</h3> : null}
            <div data-slot="sticker-grid" className="grid grid-cols-4 gap-1">
              {found.map((sticker) => (
                <Tip key={sticker.id} label={sticker.name}>
                  <button
                    type="button"
                    data-slot="sticker-button"
                    aria-label={sticker.name}
                    className="flex aspect-square items-center justify-center rounded-md p-1.5 outline-none hover:bg-muted focus-visible:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50"
                    onClick={() => onPick(sticker)}
                  >
                    <img src={sticker.url} alt="" draggable={false} loading="lazy" className="size-full object-contain" />
                  </button>
                </Tip>
              ))}
            </div>
          </section>
        ) : klipyWorkspaceId ? null : (
          <p className="px-1 pb-1 text-sm text-muted-foreground">No stickers match</p>
        )}
        {klipyWorkspaceId ? (
          <section aria-label="Stickers from KLIPY" className="not-first:mt-3">
            <h3 className="px-1 pb-1 text-xs font-medium text-muted-foreground">KLIPY</h3>
            <KlipyResults workspaceId={klipyWorkspaceId} catalog="stickers" search={search} onPick={onPickKlipy} />
          </section>
        ) : null}
      </div>
      {klipyWorkspaceId ? <PoweredByKlipy /> : null}
    </div>
  )
}
