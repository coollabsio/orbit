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
 * tinted, emoji as Twemoji, chat markdown formatted with its markers dimmed. The copy has the same characters in the same
 * font, so both lay out the same; an emoji keeps its character (transparent) and shows the image as its background, and
 * a mention or a format changes colour, background or decoration only, never width. A width that differs moves the
 * caret away from the text.
 */
export function ComposerInput({ ref, value, people, conversations, className, onScroll, ...props }: ComposerInputProps) {
  const mirror = useRef<HTMLDivElement>(null)
  const field = useRef<HTMLTextAreaElement | null>(null)

  // The textarea scrolls past the box's maximum height; the copy follows it.
  useLayoutEffect(() => {
    if (mirror.current && field.current) mirror.current.scrollTop = field.current.scrollTop
  }, [value])

  const segments = composerSegments(value, people, conversations)
  // A mention keeps its own background, so it is not part of the code or spoiler box around it.
  const fills = segments.map((segment) => (segment.kind === 'mention' ? undefined : segment.marks?.includes('spoiler') ? 'spoiler' : segment.marks?.includes('code') ? 'code' : undefined))

  return (
    <div data-slot="composer-input" className="relative flex min-h-10 shrink flex-col">
      <div
        ref={mirror}
        aria-hidden="true"
        data-slot="composer-mirror"
        // below md the base styles hold every textarea at 16px (no iOS zoom on focus); the copy must have that size too
        className={cn('pointer-events-none absolute inset-0 overflow-hidden wrap-break-word whitespace-pre-wrap text-foreground', className, 'max-md:text-[16px]!')}
      >
        {segments.map((segment, index) => {
          const marks = segment.marks
          if (segment.kind === 'text' && !marks) return segment.text
          const underline = marks?.includes('underline')
          const strike = marks?.includes('strike')
          return (
            <span
              key={index}
              data-kind={segment.kind}
              // Colour, background and decoration line each hang on one attribute: two variants that set the same property can win in the wrong order.
              data-tone={segment.kind !== 'text' ? undefined : marks?.includes('marker') ? 'marker' : marks?.includes('muted') ? 'muted' : undefined}
              data-fill={fills[index]}
              // Marker, content and marker are separate pieces of one box, so only its outer ends are rounded.
              data-fill-start={fills[index] && fills[index - 1] !== fills[index] ? '' : undefined}
              data-fill-end={fills[index] && fills[index + 1] !== fills[index] ? '' : undefined}
              data-line={underline && strike ? 'both' : underline ? 'underline' : strike ? 'strike' : undefined}
              data-strong={marks?.includes('strong') ? '' : undefined}
              data-em={marks?.includes('em') ? '' : undefined}
              // Every style here leaves the width of the text alone: the weight, the font and the size never change. Bold is a shadow, and italic is the slant the browser makes from the upright face (no italic face is loaded).
              className="data-em:italic data-fill-end:rounded-r-sm data-fill-start:rounded-l-sm data-strong:[text-shadow:0.4px_0_0_currentColor] data-[fill=code]:bg-muted data-[fill=spoiler]:bg-foreground/15 data-[kind=emoji]:bg-contain data-[kind=emoji]:bg-center data-[kind=emoji]:bg-no-repeat data-[kind=emoji]:text-transparent data-[kind=mention]:rounded-sm data-[kind=mention]:bg-primary/10 data-[kind=mention]:text-primary data-[line=both]:[text-decoration-line:underline_line-through] data-[line=strike]:line-through data-[line=underline]:underline data-[tone=marker]:text-muted-foreground/60 data-[tone=muted]:text-muted-foreground dark:data-[kind=mention]:bg-primary/20"
              style={segment.kind === 'emoji' ? { backgroundImage: `url(${segment.url})` } : undefined}
            >
              {segment.text}
            </span>
          )
        })}
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
