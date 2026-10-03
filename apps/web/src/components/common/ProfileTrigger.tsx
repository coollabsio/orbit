import { cloneElement, use, type FocusEvent, type HTMLAttributes, type KeyboardEvent, type MouseEvent, type PointerEvent, type ReactElement, type ReactNode, type Ref } from 'react'
import { cn } from 'cn'
import { ProfilePopoverContext, PROFILE_TRIGGER_ATTRIBUTE } from './profilePopover'

/** A touch that stays down this long is the long press of the row (the message action sheet), not a tap. */
const LONG_PRESS = 450

/** When the touch that is down on a trigger began. */
const touchStarts = new WeakMap<Element, number>()

type ElementProps = HTMLAttributes<HTMLElement> & { ref?: Ref<HTMLElement> }

interface ProfileTriggerProps extends Omit<ElementProps, 'children'> {
  /** Whose profile opens. Without an id (a former member, an unresolved mention) the content is not interactive. */
  userId: string | null | undefined
  /** For the label: "Open profile of Ada". */
  name: string
  /** `name` underlines on hover; `avatar` has a round focus ring; `plain` adds only the pointer (for `render` with a `Button`). */
  kind?: 'name' | 'avatar' | 'plain'
  /**
   * The caller's element becomes the trigger in place of the `<button>`. An element that is not a `<button>` gets the
   * button role and keys; use it where a `<button>` is wrong (text that must stay selectable, as a mention is).
   */
  render?: ReactElement<ElementProps>
  /** `-1` for an avatar beside a name trigger: one tab stop for each person. */
  tabIndex?: number
  /** The trigger is part of a sentence (a mention): it keeps its text as its name and is not a tab stop. */
  inText?: boolean
  children?: ReactNode
}

/** `cloneElement` that keeps the element's own children when the trigger has none. */
function withChildren(element: ReactElement<ElementProps>, props: ElementProps, children: ReactNode) {
  return children === undefined ? cloneElement(element, props) : cloneElement(element, props, children)
}

/**
 * A member's name or avatar that opens the profile popover. It is only a button: the one popover is mounted by the
 * shell, so a list can have hundreds of these. The click does not reach a row around it. Other props (a tooltip's, a
 * ref) go on to the element.
 */
export function ProfileTrigger({ userId, name, kind = 'name', inText = false, render, tabIndex, className, children, ...rest }: ProfileTriggerProps) {
  const control = use(ProfilePopoverContext)

  if (!control || !userId) {
    const plain = { ...rest, tabIndex, className: cn(render?.props.className, className) }
    return render ? withChildren(render, plain, children) : <span {...plain}>{children}</span>
  }

  const nativeButton = !render || render.type === 'button'
  const props = {
    ...rest,
    [PROFILE_TRIGGER_ATTRIBUTE]: kind,
    // a trigger inside running text (a mention) reads as its text
    ...(inText ? {} : { 'aria-label': `Open profile of ${name}` }),
    'aria-haspopup': 'dialog' as const,
    tabIndex: tabIndex ?? (nativeButton ? undefined : inText ? -1 : 0),
    className: cn(
      'cursor-pointer',
      // `plain` is an element with its own look (a `Button`)
      kind !== 'plain' && 'rounded-sm text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50 data-[profile-trigger=avatar]:rounded-full data-[profile-trigger=name]:hover-fine:hover:underline',
      render?.props.className,
      className,
    ),
    onPointerDown: (event: PointerEvent<HTMLElement>) => {
      rest.onPointerDown?.(event)
      if (event.pointerType === 'touch') touchStarts.set(event.currentTarget, event.timeStamp)
      else touchStarts.delete(event.currentTarget)
      control.prefetch(userId)
    },
    onFocus: (event: FocusEvent<HTMLElement>) => {
      rest.onFocus?.(event)
      control.prefetch(userId)
    },
    onClick: (event: MouseEvent<HTMLElement>) => {
      rest.onClick?.(event)
      event.stopPropagation()
      const touchStart = touchStarts.get(event.currentTarget)
      touchStarts.delete(event.currentTarget)
      const longPress = touchStart !== undefined && event.timeStamp - touchStart >= LONG_PRESS
      if (longPress) return
      // the end of a text selection that covers the trigger is not a click on it
      const selection = window.getSelection()
      if (selection && !selection.isCollapsed && selection.containsNode(event.currentTarget, true)) return
      // a click without a pointer (`detail` 0) is Enter or Space on a button
      control.open(userId, event.currentTarget, { name, instant: event.detail === 0 })
    },
    onKeyDown: (event: KeyboardEvent<HTMLElement>) => {
      rest.onKeyDown?.(event)
      if (event.target !== event.currentTarget || (event.key !== 'Enter' && event.key !== ' ')) return
      // a row around the trigger does not take the key as its own; a button then gets its click from the browser
      event.stopPropagation()
      if (nativeButton) return
      event.preventDefault()
      control.open(userId, event.currentTarget, { name, instant: true })
    },
  }

  if (render) return withChildren(render, { ...props, ...(nativeButton ? {} : { role: 'button' }) }, children)
  return (
    <button type="button" data-slot="profile-trigger" {...(props as HTMLAttributes<HTMLButtonElement>)}>
      {children}
    </button>
  )
}
