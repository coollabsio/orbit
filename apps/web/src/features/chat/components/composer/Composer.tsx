import {
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type ChangeEvent,
  type ClipboardEvent,
  type KeyboardEvent,
  type Ref,
  useId,
} from 'react'
import { At, Paperclip2, Send2, SmileCircle, Sticker, Xmark } from 'reicon-react'
import type { CustomStickerRecord } from '@/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { EmojiPicker } from '@/components/common/EmojiPicker'
import { MentionPopover } from '@/components/common/MentionPopover'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useCustomStickers } from '@/features/workspaces/customStickers'
import { clipboardFiles } from '@/lib/attachmentLib'
import { useMentionAutocomplete } from '@/lib/useMentionAutocomplete'
import { useChatContext } from '../../api/chatContext'
import { useSendMessage, useSendTyping } from '../../api/mutations'
import { MESSAGE_MAX_LENGTH, type Conversation, type Message } from '../../api/types'
import { clearDraft, getDraft, setDraft } from '../../lib/drafts'
import { completeEmoticon, replaceEmoticons } from '../../lib/emoticons'
import { encodeMentions, extractMentions } from '../../lib/mentionTokens'
import { quoteOf } from '../../lib/quote'
import { useCoarsePointer } from '../messages/environment'
import { conversationTitle, useChatPeople } from '../messages/people'
import { ComposerInput } from './ComposerInput'
import { ComposerUploads } from './ComposerUploads'
import { EmojiSuggestions } from './EmojiSuggestions'
import { StickerPicker } from './StickerPicker'
import { useAutosize } from './useAutosize'
import { useEmojiAutocomplete } from './useEmojiAutocomplete'
import { useUploads } from './useUploads'

/** The counter shows when this few characters are left. */
const COUNTER_FROM = 200
/** `@channel` and `@here` in a conversation with this many members ask before sending. */
const CONFIRM_FROM_MEMBERS = 6
const GLOBAL_MENTION = /(^|\s)@(channel|here)(?![\w-])/i

export interface ComposerHandle {
  focus: () => void
  /** Files dropped on the column. */
  addFiles: (files: Iterable<File>) => void
}

interface ComposerProps {
  ref?: Ref<ComposerHandle>
  conversation: Conversation
  /** Set in a thread: the message is a reply, and "Also send to #channel" shows. */
  threadRootId?: string | null
  /** The message that the next message answers: the "Replying to" bar shows. It is not kept in the draft. */
  replyTo?: Message | null
  /** The reply ended: it was cancelled (the bar's button, `Esc`) or sent. */
  onClearReply?: () => void
  /** Takes focus when it mounts (not on a touch screen, where that opens the keyboard). */
  autoFocus?: boolean
  /** `↑` in an empty composer. Returns false when there is no message to edit. */
  onEditLast?: () => boolean
  /** `Shift+Tab`. Returns false when the list has no message to focus. */
  onFocusList?: () => boolean
  onSent?: () => void
}

/**
 * The message box of a conversation or a thread: markdown source, `@`, `#` and `:emoji` autocomplete (mentions and
 * emoji show in the field as the reader will get them), files, a draft kept
 * in `localStorage`, and the typing signal. `Enter` sends (a new line on a touch screen), `Shift+Enter` is a new line.
 */
export function Composer({ ref, conversation, threadRootId = null, replyTo = null, onClearReply, autoFocus = false, onEditLast, onFocusList, onSent }: ComposerProps) {
  const { workspaceId, currentUserId } = useChatContext()
  const people = useChatPeople()
  const coarse = useCoarsePointer()
  const { send } = useSendMessage()
  const sendTyping = useSendTyping()
  const { uploads, add, remove, retry, clear } = useUploads()
  const conversationId = conversation.id
  const title = conversationTitle(conversation, people, currentUserId)
  const inThread = threadRootId !== null

  const [text, setTextState] = useState(() => getDraft(workspaceId, conversationId, threadRootId))
  const [alsoInChannel, setAlsoInChannel] = useState(false)
  const [emojiOpen, setEmojiOpen] = useState(false)
  const [stickerOpen, setStickerOpen] = useState(false)
  const stickers = useCustomStickers(workspaceId).data ?? []
  const input = useRef<HTMLTextAreaElement>(null)
  const filePicker = useRef<HTMLInputElement>(null)
  useAutosize(input, text)

  function setText(next: string) {
    setTextState(next)
    setDraft(workspaceId, conversationId, threadRootId, next)
    if (next.trim() !== '') sendTyping(conversationId, threadRootId)
  }

  // People who can be mentioned here: everybody in a public channel, the members of a private channel or a DM.
  const candidates = people.members.filter(
    (member) => !member.suspended && (conversation.kind === 'public' || conversation.memberIds.includes(member.id)),
  )
  const everyone = inThread
    ? []
    : [
        { id: '!channel', name: 'channel', handle: 'everyone here' },
        { id: '!here', name: 'here', handle: 'everyone online' },
      ]
  const mention = useMentionAutocomplete([...candidates, ...everyone], text, setText, input, undefined, undefined, people.channels)
  const emoji = useEmojiAutocomplete(text, setText, input)
  const emojiListId = useId()

  useEffect(() => {
    const element = input.current
    if (!autoFocus || !element || window.matchMedia?.('(pointer: coarse)').matches) return
    element.focus()
    element.setSelectionRange(element.value.length, element.value.length)
  }, [autoFocus])

  useImperativeHandle(ref, () => ({ focus: () => input.current?.focus(), addFiles: add }))

  // `:)` as the last word has no white space after it yet, so the body that is sent converts every emoticon
  const body = encodeMentions(replaceEmoticons(text.trim()), people.members, people.channels)
  const attachments = uploads.flatMap((upload) => (upload.attachment ? [upload.attachment] : []))
  const uploadsReady = uploads.every((upload) => upload.status === 'done')
  const remaining = MESSAGE_MAX_LENGTH - body.length
  const canSend = (body !== '' || attachments.length > 0) && uploadsReady && remaining >= 0
  const globalMention = GLOBAL_MENTION.test(text)

  async function submit() {
    if (!canSend) return
    const mentions = extractMentions(body)
    if (!inThread && (mentions.channel || mentions.here) && conversation.memberIds.length >= CONFIRM_FROM_MEMBERS) {
      const confirmed = await confirmAction({
        title: `Notify ${conversation.memberIds.length} people?`,
        description: `${mentions.channel ? '@channel' : '@here'} sends a notification to everyone in ${title}${mentions.channel ? '' : ' who is online'}.`,
        confirmLabel: 'Send',
      })
      if (!confirmed) {
        input.current?.focus()
        return
      }
    }
    send({ conversationId, threadRootId, replyTo: replyTo && quoteOf(replyTo), body, attachments, alsoInChannel: inThread && alsoInChannel })
    // The message is now a row of the list (it stays there with "Not sent" if the send fails), so the draft is done.
    setTextState('')
    clearDraft(workspaceId, conversationId, threadRootId)
    clear()
    setAlsoInChannel(false)
    onClearReply?.()
    mention.close()
    emoji.close()
    onSent?.()
    input.current?.focus()
  }

  /**
   * A sticker goes at once, as a message of its own: it answers the reply target if there is one, and leaves the text
   * and the files of the composer as they are.
   */
  function sendSticker(sticker: CustomStickerRecord) {
    setStickerOpen(false)
    send({ conversationId, threadRootId, replyTo: replyTo && quoteOf(replyTo), body: '', sticker: { id: sticker.id, name: sticker.name, url: sticker.url } })
    onClearReply?.()
    onSent?.()
  }

  function insertAtCursor(insertion: string) {
    const element = input.current
    const start = element?.selectionStart ?? text.length
    const end = element?.selectionEnd ?? text.length
    const next = `${text.slice(0, start)}${insertion}${text.slice(end)}`
    const cursor = start + insertion.length
    setText(next)
    requestAnimationFrame(() => {
      if (!element) return
      element.focus()
      element.setSelectionRange(cursor, cursor)
      mention.update(next, cursor)
    })
  }

  /** The `@` button: the list opens only after a space or at the start of a line. */
  function mentionSomeone() {
    const before = text[(input.current?.selectionStart ?? text.length) - 1]
    insertAtCursor(before === undefined || /\s/.test(before) ? '@' : ' @')
  }

  function onChange(event: ChangeEvent<HTMLTextAreaElement>) {
    const { value, selectionStart } = event.target
    // `:joy:` typed in full becomes the emoji
    if (emoji.complete(value, selectionStart)) return
    // `:D` and then a space or a new line becomes the emoji (typed, not uncovered by deleting what came after it)
    const emoticon = value.length > text.length ? completeEmoticon(value, selectionStart) : null
    if (emoticon) {
      setText(emoticon.text)
      mention.close()
      emoji.close()
      requestAnimationFrame(() => input.current?.setSelectionRange(emoticon.cursor, emoticon.cursor))
      return
    }
    setText(value)
    mention.update(value, selectionStart)
    emoji.update(value, selectionStart)
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    // `Esc` closes the mention or emoji list and goes no further; with nothing open here it reaches the view, which
    // closes the pane. It never clears the text.
    if (mention.handleKeyDown(event) || emoji.handleKeyDown(event)) {
      event.stopPropagation()
      return
    }
    // `Esc` cancels the reply first; the next one reaches the view.
    if (event.key === 'Escape' && replyTo) {
      event.stopPropagation()
      onClearReply?.()
      return
    }
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing && !coarse) {
      event.preventDefault()
      void submit()
      return
    }
    if (event.key === 'ArrowUp' && text === '' && onEditLast?.()) {
      event.preventDefault()
      return
    }
    if (event.key === 'Tab' && event.shiftKey && onFocusList?.()) event.preventDefault()
  }

  function onPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const files = clipboardFiles(event)
    // Copied cells or slides carry text and a picture of it: the text is what the user wants.
    if (files.length === 0 || event.clipboardData.getData('text/plain')) return
    event.preventDefault()
    add(files)
  }

  return (
    <div
      data-slot="composer"
      data-thread={inThread ? '' : undefined}
      // Chat merges live updates itself, so its focus must not pause the workspace's realtime refresh.
      data-realtime-safe=""
      className="shrink-0 px-4 pb-3 max-[899px]:px-2 max-[899px]:pb-2 max-[899px]:not-focus-within:pb-[max(0.5rem,env(safe-area-inset-bottom,0px))]"
    >
      <div className="relative">
        {mention.open ? (
          <MentionPopover suggestions={mention.suggestions} activeIndex={mention.activeIndex} onSelect={mention.insert} onHover={mention.setActiveIndex} />
        ) : emoji.open ? (
          <EmojiSuggestions id={emojiListId} query={emoji.query} suggestions={emoji.suggestions} activeIndex={emoji.activeIndex} onSelect={emoji.insert} onHover={emoji.setActiveIndex} />
        ) : null}
        {replyTo ? (
          <div
            data-slot="composer-reply"
            className="flex items-center gap-2 rounded-t-lg border border-b-0 border-input bg-muted/40 pl-3 text-xs text-muted-foreground"
          >
            <span className="min-w-0 flex-1 truncate">
              Replying to <span className="font-medium text-foreground">{people.byId.get(replyTo.authorId)?.name ?? 'Unknown'}</span>
            </span>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Cancel reply"
              title="Cancel reply"
              onClick={() => {
                onClearReply?.()
                input.current?.focus()
              }}
            >
              <Xmark className="size-5" />
            </Button>
          </div>
        ) : null}
        <div
          data-slot="composer-box"
          data-replying={replyTo ? '' : undefined}
          className="flex max-h-[50cqh] flex-col rounded-lg border border-input bg-background data-replying:rounded-t-none dark:bg-input/30"
        >
          <ComposerInput
            ref={input}
            value={text}
            people={people.members}
            conversations={people.channels}
            rows={1}
            aria-label={inThread ? 'Reply in Thread' : `Message ${title}`}
            aria-controls={emoji.open ? emojiListId : undefined}
            aria-activedescendant={emoji.open ? `${emojiListId}-${emoji.activeIndex}` : undefined}
            placeholder={inThread ? 'Reply…' : `Message ${title}`}
            className="min-h-10 shrink resize-none rounded-none border-0 bg-transparent px-3 pt-2.5 pb-1 text-[15px] leading-[1.5] focus-visible:ring-0 md:text-[15px] dark:bg-transparent"
            onChange={onChange}
            onKeyDown={onKeyDown}
            onPaste={onPaste}
            onSelect={(event) => {
              mention.update(event.currentTarget.value, event.currentTarget.selectionStart)
              emoji.update(event.currentTarget.value, event.currentTarget.selectionStart)
            }}
            onBlur={() => {
              mention.close()
              emoji.close()
            }}
          />
          <ComposerUploads uploads={uploads} onRemove={remove} onRetry={retry} />
          <div className="flex shrink-0 items-center gap-1 p-1.5">
            <input
              ref={filePicker}
              type="file"
              multiple
              hidden
              tabIndex={-1}
              onChange={(event) => {
                add(Array.from(event.target.files ?? []))
                event.target.value = ''
              }}
            />
            <Button variant="ghost" size="icon" aria-label="Attach files" title="Attach files" onClick={() => filePicker.current?.click()}>
              <Paperclip2 className="size-5" />
            </Button>
            <Popover open={emojiOpen} onOpenChange={setEmojiOpen} modal={false}>
              <PopoverTrigger render={<Button variant="ghost" size="icon" aria-label="Add emoji" title="Add emoji" />}>
                <SmileCircle className="size-5" />
              </PopoverTrigger>
              <PopoverContent side="top" align="start" finalFocus={input} className="w-auto gap-0 p-0">
                <EmojiPicker
                  custom
                  onPick={(emoji) => {
                    setEmojiOpen(false)
                    insertAtCursor(emoji)
                  }}
                />
              </PopoverContent>
            </Popover>
            {stickers.length > 0 ? (
              <Popover open={stickerOpen} onOpenChange={setStickerOpen} modal={false}>
                <PopoverTrigger render={<Button variant="ghost" size="icon" aria-label="Send a sticker" title="Send a sticker" />}>
                  <Sticker className="size-5" />
                </PopoverTrigger>
                {stickerOpen ? (
                  <PopoverContent side="top" align="start" finalFocus={input} className="w-auto gap-0 p-0">
                    <StickerPicker stickers={stickers} onPick={sendSticker} />
                  </PopoverContent>
                ) : null}
              </Popover>
            ) : null}
            <Button variant="ghost" size="icon" aria-label="Mention someone" title="Mention someone" onClick={mentionSomeone}>
              <At className="size-5" />
            </Button>
            <span className="flex-1" />
            {remaining <= COUNTER_FROM ? (
              <span
                data-slot="composer-counter"
                data-over={remaining < 0 ? '' : undefined}
                aria-label={remaining < 0 ? `${-remaining} characters over the limit` : `${remaining} characters left`}
                className="px-1 text-xs text-muted-foreground tabular-nums data-over:font-medium data-over:text-destructive"
              >
                {remaining}
              </span>
            ) : null}
            <Button
              variant={canSend ? 'default' : 'ghost'}
              size="icon"
              aria-label="Send message"
              title="Send message"
              disabled={!canSend}
              onClick={() => void submit()}
            >
              <Send2 className="size-5" />
            </Button>
          </div>
        </div>
      </div>
      {inThread ? (
        <div className="flex flex-col gap-1 pt-1.5 text-xs text-muted-foreground">
          <label className="flex w-fit items-center gap-2">
            <Checkbox checked={alsoInChannel} onCheckedChange={setAlsoInChannel} />
            Also send to {conversation.kind === 'dm' ? 'this conversation' : title}
          </label>
          {globalMention ? <p data-slot="composer-note">@channel and @here do nothing in a thread.</p> : null}
        </div>
      ) : null}
    </div>
  )
}
