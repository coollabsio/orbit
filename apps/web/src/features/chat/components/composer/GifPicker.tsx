import { useState } from 'react'
import { Search4 } from 'reicon-react'
import type { KlipyItem } from '@orbit/api/generated/types.gen'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { KlipyResults, PoweredByKlipy } from './KlipyResults'

/**
 * The GIF panel of the composer: a search field and GIFs from KLIPY in two columns, trending until the user searches.
 * Each GIF is a button, so `Tab` goes through them; a click picks one.
 */
export function GifPicker({ workspaceId, onPick }: { workspaceId: string; onPick: (gif: KlipyItem) => void }) {
  const [search, setSearch] = useState('')
  return (
    <div data-slot="gif-picker" className="flex h-[380px] max-h-[calc(100dvh-8rem)] w-[344px] max-w-[calc(100vw-2rem)] flex-col">
      <div className="p-2">
        <InputGroup className="border-0 bg-secondary/40 dark:bg-secondary/40">
          <InputGroupAddon>
            <Search4 />
          </InputGroupAddon>
          {/* KLIPY's rules: the search field names KLIPY */}
          <InputGroupInput autoFocus type="search" value={search} placeholder="Search KLIPY" aria-label="Search KLIPY" autoComplete="off" onChange={(event) => setSearch(event.target.value)} />
        </InputGroup>
      </div>
      <div data-slot="klipy-scroll" className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-2">
        <KlipyResults workspaceId={workspaceId} catalog="gifs" search={search} onPick={onPick} />
      </div>
      <PoweredByKlipy />
    </div>
  )
}
