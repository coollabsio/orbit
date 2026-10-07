import { EmojiPicker as Frimousse, type EmojiPickerListCategoryHeaderProps, type EmojiPickerListEmojiProps, type EmojiPickerListRowProps } from 'frimousse'
import { useContext, useState } from 'react'
import { SearchNormal as Search } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { Spinner } from '@/components/ui/spinner'
import { CustomEmojiContext } from '@/lib/customEmojiContext'
import { EMOJIBASE_PATH, EMOJI_VERSION, normalizeEmoji } from '@/lib/twemoji'
import { Emoji } from './Emoji'

function CategoryHeader({ category, ...props }: EmojiPickerListCategoryHeaderProps) {
  return (
    <div data-slot="emoji-category" className="bg-popover px-3 pt-3 pb-1.5 text-xs font-medium text-muted-foreground" {...props}>
      {category.label}
    </div>
  )
}

function Row({ children, ...props }: EmojiPickerListRowProps) {
  return (
    <div data-slot="emoji-row" className="scroll-my-1.5 px-1.5" {...props}>
      {children}
    </div>
  )
}

function EmojiButton({ emoji, ...props }: EmojiPickerListEmojiProps) {
  return (
    <button data-slot="emoji-button" className="flex size-9 items-center justify-center rounded-md text-xl outline-none data-active:bg-muted" {...props}>
      <Emoji value={emoji.emoji} />
    </button>
  )
}

/**
 * The emoji picker panel: search, categories, keyboard navigation and skin tones (Frimousse), with Twemoji images.
 * The emoji data comes from Orbit itself, not from a CDN. `onRemove` adds a Remove action (page and callout icons).
 * `custom` (chat only) adds the workspace's custom emoji in a group above the list; one of them is picked as the text
 * `:name:`. Frimousse has no place for entries that are not characters, so the group is ours and the search filters it.
 */
export function EmojiPicker({ onPick, onRemove, custom = false }: { onPick: (emoji: string) => void; onRemove?: () => void; custom?: boolean }) {
  const customEmoji = useContext(CustomEmojiContext)
  const [search, setSearch] = useState('')
  const query = search.trim().toLowerCase()
  const customNames = custom ? [...customEmoji.keys()].filter((name) => name.includes(query)) : []
  return (
    <Frimousse.Root
      data-slot="emoji-picker"
      columns={9}
      locale="en"
      emojiVersion={EMOJI_VERSION}
      emojibaseUrl={EMOJIBASE_PATH}
      className="isolate flex h-[380px] w-fit flex-col"
      onEmojiSelect={({ emoji }) => onPick(normalizeEmoji(emoji))}
    >
      <div className="flex items-center gap-2 p-2 pb-0">
        <label className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-md bg-secondary/60 px-2.5 text-muted-foreground focus-within:ring-3 focus-within:ring-ring/50">
          <Search className="size-3.5 shrink-0" aria-hidden="true" />
          <Frimousse.Search autoFocus placeholder="Search emoji" aria-label="Search emoji" onChange={(event) => setSearch(event.target.value)} className="min-w-0 flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground" />
        </label>
        {onRemove ? (
          <Button type="button" variant="ghost" size="sm" onClick={onRemove}>
            Remove
          </Button>
        ) : null}
      </div>
      {customNames.length > 0 ? (
        <div data-slot="emoji-custom" role="group" aria-label="Custom" className="max-h-[7.5rem] shrink-0 overflow-y-auto overscroll-contain border-b pb-1.5">
          <div data-slot="emoji-category" className="sticky top-0 bg-popover px-3 pt-3 pb-1.5 text-xs font-medium text-muted-foreground">Custom</div>
          {/* a grid of nine, as wide as a row of the list: it must not widen the panel */}
          <div className="grid grid-cols-9 px-1.5">
            {customNames.map((name) => (
              <button
                key={name}
                type="button"
                data-slot="emoji-button"
                aria-label={`:${name}:`}
                className="flex size-9 items-center justify-center rounded-md text-xl outline-none hover:bg-muted focus-visible:bg-muted"
                onClick={() => onPick(`:${name}:`)}
              >
                <Emoji value={`:${name}:`} />
              </button>
            ))}
          </div>
        </div>
      ) : null}
      <Frimousse.Viewport className="relative min-h-0 flex-1 outline-none">
        <Frimousse.Loading className="absolute inset-0 flex items-center justify-center">
          <Spinner className="text-muted-foreground" />
        </Frimousse.Loading>
        <Frimousse.Empty className="absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">No emoji found</Frimousse.Empty>
        <Frimousse.List className="pb-1.5 select-none" components={{ CategoryHeader, Row, Emoji: EmojiButton }} />
      </Frimousse.Viewport>
      <div className="flex h-11 shrink-0 items-center gap-2 border-t px-3">
        <Frimousse.ActiveEmoji>
          {({ emoji }) =>
            emoji ? (
              <>
                <span className="text-xl">
                  <Emoji value={emoji.emoji} />
                </span>
                <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{emoji.label}</span>
              </>
            ) : (
              <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">Pick an emoji</span>
            )
          }
        </Frimousse.ActiveEmoji>
        <Frimousse.SkinTone>
          {({ skinTone, setSkinTone, skinToneVariations }) => {
            const index = skinToneVariations.findIndex((variation) => variation.skinTone === skinTone)
            const current = skinToneVariations[index] ?? skinToneVariations[0]
            const next = skinToneVariations[(index + 1) % skinToneVariations.length]
            if (!current || !next) return null
            return (
              <Tip label="Change skin tone">
                <Button type="button" variant="ghost" size="icon-sm" className="text-base" aria-label={`Skin tone: ${skinTone}. Change skin tone`} onClick={() => setSkinTone(next.skinTone)}>
                  <Emoji value={current.emoji} />
                </Button>
              </Tip>
            )
          }}
        </Frimousse.SkinTone>
      </div>
    </Frimousse.Root>
  )
}
