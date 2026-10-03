import { useLayoutEffect, useRef, type ComponentProps, type Ref } from 'react'
import { cn } from 'cn'
import { Textarea } from '@/components/ui/textarea'
import type { MentionTarget } from '../../lib/mentionTokens'
import { composerSegments } from './composerSegments'

interface ComposerInputProps extends Omit<ComponentProps<typeof Textarea>, 'value'> {
  ref?: Ref<HTMLTextAreaElement>
  value: string
  people: readonly MentionTarget[]
  conversations: readonly MentionTarget[]
}

/**
 * The text field of the message box. It is a plain textarea, so typing, selection, undo, IME and spell check are the
 * browser's own. Its text is transparent, and a copy of the text behind it shows what the reader will get: mentions
 * tinted, emoji as Twemoji. The copy has the same characters in the same font, so both lay out the same; an emoji keeps
 * its character (transparent) and shows the image as its background, and a mention changes colour only, never width.
 */
export function ComposerInput({ ref, value, people, conversations, className, onScroll, ...props }: ComposerInputProps) {
  const mirror = useRef<HTMLDivElement>(null)
  const field = useRef<HTMLTextAreaElement | null>(null)

  // The textarea scrolls past the box's maximum height; the copy follows it.
  useLayoutEffect(() => {
    if (mirror.current && field.current) mirror.current.scrollTop = field.current.scrollTop
  }, [value])

  return (
    <div data-slot="composer-input" className="relative flex min-h-10 shrink flex-col">
      <div
        ref={mirror}
        aria-hidden="true"
        data-slot="composer-mirror"
        className={cn('pointer-events-none absolute inset-0 overflow-hidden wrap-break-word whitespace-pre-wrap text-foreground', className)}
      >
        {composerSegments(value, people, conversations).map((segment, index) =>
          segment.kind === 'mention' ? (
            <span key={index} className="rounded-sm bg-primary/10 text-primary dark:bg-primary/20">
              {segment.text}
            </span>
          ) : segment.kind === 'emoji' ? (
            <span key={index} className="bg-contain bg-center bg-no-repeat text-transparent" style={{ backgroundImage: `url(${segment.url})` }}>
              {segment.text}
            </span>
          ) : (
            segment.text
          ),
        )}
        {/* a last empty line has no height without a character on it */}
        {'\u200b'}
      </div>
      <Textarea
        ref={(element) => {
          field.current = element
          if (typeof ref === 'function') ref(element)
          else if (ref) ref.current = element
        }}
        value={value}
        className={cn('relative text-transparent caret-foreground [scrollbar-width:none] selection:bg-primary/30 selection:text-transparent', className)}
        onScroll={(event) => {
          if (mirror.current) mirror.current.scrollTop = event.currentTarget.scrollTop
          onScroll?.(event)
        }}
        {...props}
      />
    </div>
  )
}
