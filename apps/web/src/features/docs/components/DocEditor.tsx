import { useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { ArrowLeft, MoreH as Ellipsis, Trash as Trash2 } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { relativeTime } from '@/lib/format'
import { createDoc, updateDocContent, updateDocCover, updateDocIcon, updateDocTitle } from '@/mock/actions'
import { nextId } from '@/mock/store'
import type { Doc, DocBlock, User } from '@/mock/types'
import { ancestorsOf, numberedIndex } from '@/features/docs/docsLib'
import { fileToAttachment } from '@/lib/attachmentLib'
import { ConfirmDeleteModal } from '@/components/common/ConfirmDeleteModal'
import { descendantsOf } from '@/features/docs/docsLib'
import { BlockEditor } from './BlockEditor'
import { CoverBanner } from './CoverBanner'
import { MediaBlock } from './MediaBlock'
import { Emoji } from '@/components/common/Emoji'
import { CoverSourcePanel } from './CoverSourcePanel'
import { BlockView } from './BlockView'
import { buildMentionTokens } from '@/lib/mentions'
import { PageBlock } from './PageBlock'
import { PageLinkDialog } from './PageLinkDialog'

// data-danger (not variant="destructive"): the preset menu popup forces destructive items to the accent color.
const menuItemClass =
  'min-h-8 gap-2 rounded-md px-2 py-1.5 text-sm leading-5 text-foreground focus:bg-muted data-[danger=true]:text-destructive data-[danger=true]:focus:bg-muted data-[danger=true]:focus:text-destructive'
/** The emoji picker panel brings its own chrome, so the popover is just an anchored frame. */
const emojiPanelClass = 'w-auto gap-0 rounded-xl border border-border bg-popover p-0 text-foreground shadow-xl ring-0'

interface DocEditorProps {
  doc: Doc
  docs: Doc[]
  users: User[]
  onDelete: () => void
}

export function DocEditor({ doc, docs, users, onDelete }: DocEditorProps) {
  const navigate = useNavigate()
  const [title, setTitle] = useState(doc.title)
  const [editingId, setEditingId] = useState<string | null>(null)
  // "Link a page" dialog opened from the slash menu; holds the block to replace.
  const [linkTarget, setLinkTarget] = useState<string | null>(null)
  // "Add cover" panel for a page without a cover (with a cover, the banner hosts its own panel)
  const [coverPanelOpen, setCoverPanelOpen] = useState(false)
  const [iconPickerOpen, setIconPickerOpen] = useState(false)
  const [addIconOpen, setAddIconOpen] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  // "/image" and "/file": the hidden picker fills this block (or replaces it when empty)
  const mediaInput = useRef<HTMLInputElement>(null)
  const [mediaTarget, setMediaTarget] = useState<{ blockId: string; kind: 'image' | 'file' } | null>(null)

  const ancestors = ancestorsOf(docs, doc.id)
  const updatedBy = users.find((u) => u.id === doc.updatedBy)
  const mentionTokens = buildMentionTokens(users)

  const commitTitle = () => {
    const next = title.trim() || 'Untitled'
    if (next !== doc.title) updateDocTitle(doc.id, next)
    if (next !== title) setTitle(next)
  }

  const setBlocks = (blocks: DocBlock[]) => updateDocContent(doc.id, blocks)

  const mediaBlockFor = (file: File, kind: 'image' | 'file' | 'auto'): DocBlock => {
    const att = fileToAttachment(file)
    const isImage = att.mimeType.startsWith('image/') && kind !== 'file'
    return {
      id: nextId('db'),
      type: isImage ? 'image' : 'file',
      text: '',
      url: att.url,
      fileName: att.fileName,
      fileSize: att.fileSize,
      mimeType: att.mimeType,
    }
  }

  /** Pasted or dropped files become media blocks after `blockId` (null = end of the page). */
  const insertFilesAfter = (blockId: string | null, files: File[], kind: 'image' | 'file' | 'auto' = 'auto') => {
    if (files.length === 0) return
    const fresh = files.map((file) => mediaBlockFor(file, kind))
    const at = blockId ? doc.content.findIndex((b) => b.id === blockId) : -1
    const blocks =
      at === -1 ? [...doc.content, ...fresh] : [...doc.content.slice(0, at + 1), ...fresh, ...doc.content.slice(at + 1)]
    setBlocks(blocks)
  }

  const pickMedia = (blockId: string, kind: 'image' | 'file') => {
    setMediaTarget({ blockId, kind })
    requestAnimationFrame(() => mediaInput.current?.click())
  }

  const handlePickedMedia = (files: FileList | null) => {
    if (!mediaTarget || !files || files.length === 0) {
      setMediaTarget(null)
      return
    }
    const fresh = Array.from(files).map((file) => mediaBlockFor(file, mediaTarget.kind))
    const at = doc.content.findIndex((b) => b.id === mediaTarget.blockId)
    const target = doc.content[at]
    // an empty paragraph that only held the "/image" trigger is replaced, not kept
    const replace = target && target.type === 'p' && target.text.trim() === ''
    const blocks =
      at === -1
        ? [...doc.content, ...fresh]
        : [...doc.content.slice(0, at + (replace ? 0 : 1)), ...fresh, ...doc.content.slice(at + 1)]
    setBlocks(blocks)
    setEditingId(null)
    setMediaTarget(null)
  }

  const commitBlock = (id: string, text: string, action: 'close' | 'insert') => {
    let blocks = doc.content.map((b) => (b.id === id ? { ...b, text } : b))
    if (action === 'insert') {
      const fresh: DocBlock = { id: nextId('db'), type: 'p', text: '' }
      const at = blocks.findIndex((b) => b.id === id)
      blocks = [...blocks.slice(0, at + 1), fresh, ...blocks.slice(at + 1)]
      setEditingId(fresh.id)
    } else {
      setEditingId(null)
    }
    setBlocks(blocks)
  }

  const removeBlock = (id: string) => {
    if (doc.content.length === 1) return
    setEditingId(null)
    setBlocks(doc.content.filter((b) => b.id !== id))
  }

  const removeEmptyBlock = (id: string) => {
    const index = doc.content.findIndex((block) => block.id === id)
    const previous = doc.content
      .slice(0, index)
      .reverse()
      .find((block) => block.type !== 'page' && block.type !== 'divider')

    if (!previous) return

    setBlocks(doc.content.filter((block) => block.id !== id))
    setEditingId(previous.id)
  }

  const changeType = (id: string, type: DocBlock['type'], text: string) => {
    setBlocks(
      doc.content.map((b) =>
        b.id === id ? { ...b, type, text, checked: type === 'todo' ? (b.checked ?? false) : undefined } : b,
      ),
    )
  }

  const toggleTodo = (id: string) => {
    setBlocks(doc.content.map((b) => (b.id === id ? { ...b, checked: !b.checked } : b)))
  }

  const addBlock = () => {
    const last = doc.content.at(-1)
    if (last?.type === 'p' && last.text === '') {
      setEditingId(last.id)
      return
    }
    const fresh: DocBlock = { id: nextId('db'), type: 'p', text: '' }
    setBlocks([...doc.content, fresh])
    setEditingId(fresh.id)
  }

  /** The docs reference insertSubpage: new child page, current block becomes a page block, then open it. */
  const insertSubpage = (blockId: string) => {
    const child = createDoc(doc.id)
    setEditingId(null)
    setBlocks(
      doc.content.map((b) =>
        b.id === blockId ? { id: b.id, type: 'page' as const, text: 'New page', refId: child.id } : b,
      ),
    )
    navigate(`/docs/${child.id}`)
  }

  /** The docs reference "Link a page": remember the "/" block and open the picker. */
  const openLinkDialog = (blockId: string) => {
    setEditingId(null)
    setLinkTarget(blockId)
  }

  const pickLinkedPage = (picked: Doc) => {
    if (linkTarget) {
      setBlocks(
        doc.content.map((b) =>
          b.id === linkTarget ? { id: b.id, type: 'page' as const, text: picked.title, refId: picked.id } : b,
        ),
      )
    }
    setLinkTarget(null)
  }

  return (
    <section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background max-[899px]:group-data-[view=index]/docs:hidden">
      <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3 max-[899px]:border-b-0">
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="hidden text-muted-foreground/70 max-[899px]:inline-flex"
          aria-label="Back to docs"
          onClick={() => navigate('/docs')}
        >
          <ArrowLeft className="size-4" />
        </Button>
        <nav className="flex min-w-0 items-center gap-1.5 overflow-hidden text-[13px] text-muted-foreground/70" aria-label="Page path">
          {ancestors.map((a) => (
            <span key={a.id} className="flex min-w-0 items-center gap-1.5">
              <Link className="block min-w-0 truncate rounded-[4px] text-muted-foreground/70 no-underline hover:text-foreground" to={`/docs/${a.id}`}>
                {a.title || 'Untitled'}
              </Link>
              <span className="shrink-0">/</span>
            </span>
          ))}
          <Link
            className="block min-w-0 shrink truncate rounded-[4px] font-medium text-foreground no-underline"
            to={`/docs/${doc.id}`}
            aria-current="page"
          >
            {title || 'Untitled'}
          </Link>
        </nav>
        <span className="flex-1" />
        <span className="whitespace-nowrap text-xs text-muted-foreground/70 max-[899px]:hidden">
          Updated {relativeTime(doc.updatedAt)} by {updatedBy?.name ?? 'Unknown'}
        </span>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button type="button" variant="ghost" size="icon-sm" className="text-muted-foreground/70" aria-label="Document options" />
            }
          >
            <Ellipsis className="size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-auto min-w-32">
            <DropdownMenuItem className={menuItemClass} data-danger="true" onClick={() => setConfirmDelete(true)}>
              <Trash2 className="size-[14px]" />
              Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-6 pt-8 pb-24 max-[899px]:px-3 max-[899px]:pt-[18px] max-[899px]:pb-14">
        {doc.cover ? <CoverBanner key={`${doc.id}:${doc.cover}:${doc.coverPos ?? ''}`} doc={doc} /> : null}
        <div className="mx-auto max-w-[760px]">
          {doc.icon ? (
            <div className="relative z-[2] w-fit data-[cover=true]:mt-[-52px]" data-cover={doc.cover ? 'true' : undefined}>
              <Popover open={iconPickerOpen} onOpenChange={setIconPickerOpen}>
                <PopoverTrigger
                  render={
                    <Button
                      type="button"
                      variant="ghost"
                      className="h-auto cursor-pointer rounded-xl border-0 p-0.5 text-[60px] leading-none font-normal drop-shadow-[0_1px_2px_rgb(0_0_0/0.3)] hover:bg-foreground/[0.02] dark:hover:bg-foreground/[0.02]"
                      aria-label="Change icon"
                    />
                  }
                >
                  <Emoji value={doc.icon ?? ''} size={56} />
                </PopoverTrigger>
                <PopoverContent align="start" className={emojiPanelClass}>
                  <EmojiPicker
                    onPick={(emoji) => {
                      updateDocIcon(doc.id, emoji)
                      setIconPickerOpen(false)
                    }}
                    onRemove={() => {
                      updateDocIcon(doc.id, null)
                      setIconPickerOpen(false)
                    }}
                  />
                </PopoverContent>
              </Popover>
            </div>
          ) : null}
          {!doc.icon || !doc.cover ? (
            <div className="flex gap-2 pt-1.5 pb-2.5">
              {!doc.icon ? (
                <Popover open={addIconOpen} onOpenChange={setAddIconOpen}>
                  <PopoverTrigger
                    render={<Button type="button" variant="ghost" size="sm" className="text-muted-foreground/70 hover:text-foreground" />}
                  >
                    😀 Add icon
                  </PopoverTrigger>
                  <PopoverContent align="start" className={emojiPanelClass}>
                    <EmojiPicker
                      onPick={(emoji) => {
                        updateDocIcon(doc.id, emoji)
                        setAddIconOpen(false)
                      }}
                    />
                  </PopoverContent>
                </Popover>
              ) : null}
              {!doc.cover ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="text-muted-foreground/70 hover:text-foreground"
                  onClick={() => setCoverPanelOpen((o) => !o)}
                >
                  🖼️ Add cover
                </Button>
              ) : null}
            </div>
          ) : null}
          {!doc.cover && coverPanelOpen ? (
            <div className="mb-3 max-w-[420px] rounded-lg border border-border p-3">
              <CoverSourcePanel
                onPicked={(url) => {
                  setCoverPanelOpen(false)
                  updateDocCover(doc.id, { cover: url, coverPos: null })
                }}
              />
            </div>
          ) : null}
          <Input
            className="mb-5 h-auto w-full rounded-none border-0 bg-transparent p-0 text-[30px] leading-[1.25] font-bold text-foreground shadow-none outline-none placeholder:text-muted-foreground/70 focus:outline-none focus-visible:ring-0 md:text-[30px] max-[899px]:mb-3.5 max-[899px]:text-[22px]! dark:bg-transparent"
            value={title}
            placeholder="Untitled"
            onChange={(e) => setTitle(e.target.value)}
            onBlur={commitTitle}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                const firstWritable = doc.content.find(
                  (block, index) =>
                    !(index === 0 && block.type === 'h1' && block.text === doc.title) &&
                    block.type !== 'page' &&
                    block.type !== 'divider' &&
                    block.type !== 'image' &&
                    block.type !== 'file' &&
                    !(block.type === 'embed' && block.text.trim() !== ''),
                )
                setEditingId(firstWritable?.id ?? null)
                e.currentTarget.blur()
              }
            }}
          />
          <div
            className="flex flex-col text-[15px] leading-[1.7] text-foreground max-[899px]:text-xs max-[899px]:leading-[17px]"
            onDragOver={(e) => {
              if (e.dataTransfer.types.includes('Files')) e.preventDefault()
            }}
            onDrop={(e) => {
              if (!e.dataTransfer.types.includes('Files')) return
              e.preventDefault()
              insertFilesAfter(null, Array.from(e.dataTransfer.files))
            }}
          >
            {doc.content.map((block, index) =>
              index === 0 && block.type === 'h1' && block.text === doc.title ? null : block.type === 'page' ? (
                <PageBlock
                  key={block.id}
                  block={block}
                  docs={docs}
                  onOpen={(id) => navigate(`/docs/${id}`)}
                  onRemove={() => removeBlock(block.id)}
                />
              ) : block.type === 'image' || block.type === 'file' || (block.type === 'embed' && block.text.trim() !== '') ? (
                <MediaBlock key={block.id} block={block} onRemove={() => removeBlock(block.id)} />
              ) : block.type === 'divider' ? (
                <BlockView mentionTokens={mentionTokens}
                  key={block.id}
                  block={block}
                  blocks={doc.content}
                  index={index}
                  onEdit={() => undefined}
                  onToggleTodo={() => undefined}
                />
              ) : block.id === editingId ? (
                <BlockEditor users={users}
                  key={block.id}
                  block={block}
                  autoFocus
                  onCommit={(text, action) => commitBlock(block.id, text, action)}
                  onDelete={() => removeEmptyBlock(block.id)}
                  onChangeType={(type, text) => changeType(block.id, type, text)}
                  onToggleTodo={() => toggleTodo(block.id)}
                  marker={
                    block.type === 'bullet'
                      ? '•'
                      : block.type === 'numbered'
                        ? `${numberedIndex(doc.content, index)}.`
                        : undefined
                  }
                  onSubpage={() => insertSubpage(block.id)}
                  onLinkPage={() => openLinkDialog(block.id)}
                  onPickMedia={(kind) => pickMedia(block.id, kind)}
                  onFiles={(files) => insertFilesAfter(block.id, files)}
                />
              ) : (
                <BlockView mentionTokens={mentionTokens}
                  key={block.id}
                  block={block}
                  blocks={doc.content}
                  index={index}
                  onEdit={() => setEditingId(block.id)}
                  onToggleTodo={() => toggleTodo(block.id)}
                />
              ),
            )}
          </div>
          <Button
            type="button"
            variant="ghost"
            className="block h-auto min-h-40 w-full cursor-text rounded-none border-0 p-0 hover:bg-transparent dark:hover:bg-transparent"
            aria-label="Continue writing"
            onClick={addBlock}
          />
          <input
            ref={mediaInput}
            type="file"
            multiple
            hidden
            accept={mediaTarget?.kind === 'image' ? 'image/*' : undefined}
            aria-label="Upload media"
            onChange={(e) => {
              handlePickedMedia(e.target.files)
              e.target.value = ''
            }}
          />
        </div>
      </div>
      {linkTarget ? (
        <PageLinkDialog
          docs={docs}
          excludeId={doc.id}
          onPick={pickLinkedPage}
          onClose={() => setLinkTarget(null)}
        />
      ) : null}
      {confirmDelete ? (
        <ConfirmDeleteModal
          title={`Delete "${doc.title || 'Untitled'}"?`}
          description={
            descendantsOf(docs, doc.id).length > 0
              ? `This will permanently delete the page and its ${descendantsOf(docs, doc.id).length} sub-page${descendantsOf(docs, doc.id).length === 1 ? '' : 's'}.`
              : 'This will permanently delete the page.'
          }
          onClose={() => setConfirmDelete(false)}
          onConfirm={() => {
            setConfirmDelete(false)
            onDelete()
          }}
        />
      ) : null}
    </section>
  )
}
