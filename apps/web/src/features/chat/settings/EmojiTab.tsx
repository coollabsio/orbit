// Chat settings › Emoji: upload custom emojis usable across the whole app as :name:.
import { useRef, useState } from 'react'
import { Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { createCustomEmoji, deleteCustomEmoji, renameCustomEmoji } from '../../../mock/actions'
import { useAppState } from '../../../mock/store'
import type { CustomEmoji } from '../../../mock/types'
import { ConfirmDeleteModal } from '../components/ChannelModals'

const MAX_EMOJI_BYTES = 256 * 1024

/** File name → shortcode: lowercase, [a-z0-9_], no extension. */
function slugify(fileName: string): string {
  return fileName
    .replace(/\.[^.]+$/, '')
    .toLowerCase()
    .replace(/[^a-z0-9_+-]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 32)
}

export function EmojiTab() {
  const state = useAppState()
  const fileInput = useRef<HTMLInputElement>(null)
  const [error, setError] = useState('')
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [deleteTarget, setDeleteTarget] = useState<CustomEmoji | null>(null)

  const upload = (files: FileList | null) => {
    if (!files) return
    setError('')
    for (const file of Array.from(files)) {
      if (!file.type.startsWith('image/')) {
        setError('Emojis must be images.')
        continue
      }
      if (file.size > MAX_EMOJI_BYTES) {
        setError('Emojis must be 256KB or smaller.')
        continue
      }
      createCustomEmoji(slugify(file.name) || 'emoji', URL.createObjectURL(file))
    }
  }

  const commitName = (emoji: CustomEmoji) => {
    const draft = drafts[emoji.id]
    if (draft === undefined) return
    const name = slugify(`${draft}.x`)
    if (name && name !== emoji.name) renameCustomEmoji(emoji.id, name)
    setDrafts((prev) => {
      const { [emoji.id]: _discard, ...rest } = prev
      return rest
    })
  }

  return (
    <div className="flex flex-col gap-8">
      <div className="flex flex-col items-start gap-4 border-b border-border pb-7">
        <div>
          <h2 className="text-xl leading-7 font-semibold text-foreground">Emoji</h2>
          <p className="mt-3 max-w-[576px] text-sm leading-6 text-foreground">
            Custom emojis work everywhere in the app: type :name: in chat, comments and docs, or pick them
            from any emoji panel.
          </p>
        </div>
        <Button size="lg" onClick={() => fileInput.current?.click()}>
          Upload Emoji
        </Button>
        <input
          ref={fileInput}
          type="file"
          accept="image/*"
          multiple
          hidden
          aria-label="Upload emoji images"
          onChange={(e) => {
            upload(e.target.files)
            e.target.value = ''
          }}
        />
      </div>

      {error ? <p className="rounded-lg bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p> : null}

      {state.customEmojis.length === 0 ? (
        <p className="py-8 text-sm text-muted-foreground">No custom emojis yet.</p>
      ) : (
        <div className="flex flex-col">
          <div className="grid grid-cols-[72px_minmax(0,1fr)_40px] items-center gap-3 border-b border-border px-1 pt-2.5 pb-1.5 text-[11px] font-semibold tracking-[0.03em] uppercase text-muted-foreground/70">
            <span>Emoji</span>
            <span>Name</span>
            <span />
          </div>
          {state.customEmojis.map((emoji) => (
            <div key={emoji.id} className="grid grid-cols-[72px_minmax(0,1fr)_40px] items-center gap-3 border-b border-border px-1 py-2.5">
              <img className="size-8 rounded-md object-contain" src={emoji.url} alt={`:${emoji.name}:`} />
              <div className="flex items-center gap-0.5 text-muted-foreground/70">
                <Input
                  className="h-8 w-[200px]"
                  value={drafts[emoji.id] ?? emoji.name}
                  aria-label={`Emoji name ${emoji.name}`}
                  onChange={(e) => setDrafts((prev) => ({ ...prev, [emoji.id]: e.target.value }))}
                  onBlur={() => commitName(emoji)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') e.currentTarget.blur()
                  }}
                />
              </div>
              <Button
                variant="ghost"
                size="icon-sm"
                className="text-muted-foreground/70 hover:text-destructive"
                aria-label={`Delete :${emoji.name}:`}
                title="Delete"
                onClick={() => setDeleteTarget(emoji)}
              >
                <Trash2 className="size-4" />
              </Button>
            </div>
          ))}
        </div>
      )}

      {deleteTarget ? (
        <ConfirmDeleteModal
          title={`Delete :${deleteTarget.name}:?`}
          description="Messages that use this emoji will show the plain :name: text instead."
          onClose={() => setDeleteTarget(null)}
          onConfirm={() => {
            deleteCustomEmoji(deleteTarget.id)
            setDeleteTarget(null)
          }}
        />
      ) : null}
    </div>
  )
}
