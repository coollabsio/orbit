import { useState } from 'react'
import { SearchNormal } from 'reicon-react'
import type { CustomStickerRecord } from '@/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { searchStickers } from '@/lib/customSticker'

/**
 * The sticker panel of the composer: a search field and the workspace's stickers, four in a row. Each is a button, so
 * `Tab` goes through them; a click picks one.
 */
export function StickerPicker({ stickers, onPick }: { stickers: readonly CustomStickerRecord[]; onPick: (sticker: CustomStickerRecord) => void }) {
  const [search, setSearch] = useState('')
  const found = searchStickers(stickers, search)
  return (
    <div data-slot="sticker-picker" className="flex max-h-[380px] w-[344px] max-w-[calc(100vw-2rem)] flex-col">
      <div className="p-2">
        <InputGroup className="border-0 bg-secondary/40 dark:bg-secondary/40">
          <InputGroupAddon>
            <SearchNormal />
          </InputGroupAddon>
          <InputGroupInput autoFocus type="search" value={search} placeholder="Search stickers" aria-label="Search stickers" autoComplete="off" onChange={(event) => setSearch(event.target.value)} />
        </InputGroup>
      </div>
      {found.length > 0 ? (
        <div data-slot="sticker-grid" className="grid min-h-0 grid-cols-4 gap-1 overflow-y-auto overscroll-contain px-2 pb-2">
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
      ) : (
        <p className="px-3 pb-3 text-sm text-muted-foreground">No stickers match</p>
      )}
    </div>
  )
}
