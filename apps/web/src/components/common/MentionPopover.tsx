import { Hashtag as Hash, User } from 'reicon-react'
import type { MentionSuggestion } from '@/lib/useMentionAutocomplete'
import { Button } from '@/components/ui/button'
import { TaskStatusIcon } from '@/features/tasks/components/TaskStatusIcon'
import { UserAvatar } from './UserAvatar'
import { cn } from 'cn'

const SECTION: Record<MentionSuggestion['kind'], string> = { user: 'People', channel: 'Channels', task: 'Tasks' }

/**
 * the chat reference mention popup: people (avatar, @label, handle), tasks (status, ID, title, project) and channels,
 * each under its heading; onMouseDown keeps the textarea focused.
 */
export function MentionPopover({
  suggestions,
  activeIndex,
  onSelect,
  onHover,
  placement = 'above',
  showHandle = true,
  hint,
}: {
  suggestions: MentionSuggestion[]
  activeIndex: number
  onSelect: (suggestion: MentionSuggestion) => void
  onHover: (index: number) => void
  placement?: 'above' | 'below'
  /** Docs hide the right-side handle column. */
  showHandle?: boolean
  /** A line under the list, e.g. what else typing finds. */
  hint?: string
}) {
  if (suggestions.length === 0) return null
  return (
    <div
      className={cn(
        'absolute z-50 max-h-80 overflow-y-auto overscroll-contain border border-border bg-popover py-2 shadow-xl',
        placement === 'below' ? 'top-[calc(100%+6px)] left-[30px] w-[min(340px,calc(100vw-48px))] rounded-[10px]' : 'right-0 bottom-full left-0 mb-3 rounded-lg',
      )}
      data-placement={placement}
    >
      {suggestions.map((suggestion, index) => {
        const section = SECTION[suggestion.kind]
        const heading = index === 0 || SECTION[suggestions[index - 1]!.kind] !== section
        return (
          <div key={suggestion.id} className={cn(heading && index > 0 && 'mt-1.5')}>
            {heading ? <div className="px-3 pb-1 text-xs font-bold tracking-wider text-muted-foreground uppercase">{section}</div> : null}
            <Button
              type="button"
              variant="ghost"
              className="h-auto w-full justify-start gap-3 rounded-none px-3 py-2 text-left font-normal data-[active=true]:bg-muted"
              data-active={index === activeIndex ? 'true' : undefined}
              data-kind={suggestion.kind}
              onMouseDown={(event) => {
                event.preventDefault()
                onSelect(suggestion)
              }}
              onMouseEnter={() => onHover(index)}
            >
              {suggestion.task ? <TaskRow task={suggestion.task} /> : <PersonOrChannelRow suggestion={suggestion} showHandle={showHandle} />}
            </Button>
          </div>
        )
      })}
      {hint ? <div className="border-t border-border/60 px-3 pt-2 mt-1.5 text-xs text-muted-foreground">{hint}</div> : null}
    </div>
  )
}

function TaskRow({ task }: { task: NonNullable<MentionSuggestion['task']> }) {
  return (
    <>
      <span className="grid size-7 shrink-0 place-items-center">
        <TaskStatusIcon status={task.status} size={14} />
      </span>
      <span className="shrink-0 text-xs font-medium text-muted-foreground tabular-nums">{task.identifier}</span>
      <span className="min-w-0 flex-1 truncate text-sm font-semibold">{task.title}</span>
      {task.projectName ? <span className="max-w-[35%] shrink-0 truncate text-xs text-muted-foreground">{task.projectName}</span> : null}
    </>
  )
}

function PersonOrChannelRow({ suggestion, showHandle }: { suggestion: MentionSuggestion; showHandle: boolean }) {
  return (
    <>
      {suggestion.avatar ? (
        <UserAvatar user={suggestion.avatar} size={28} className="shrink-0" />
      ) : (
        <span className="grid size-7 shrink-0 place-items-center rounded-full bg-muted text-xs font-bold text-muted-foreground">
          {suggestion.kind === 'channel' ? <Hash className="size-[13px]" /> : suggestion.label.charAt(0).toUpperCase()}
        </span>
      )}
      <span className="min-w-0 flex-1 truncate text-sm font-semibold" style={suggestion.color ? { color: suggestion.color } : undefined}>
        {suggestion.kind === 'channel' ? suggestion.label : `@${suggestion.label}`}
      </span>
      {suggestion.kind === 'user' && showHandle ? (
        <span className="flex items-center gap-1 text-xs font-medium text-muted-foreground">
          <User className="size-3" />
          {suggestion.username}
        </span>
      ) : null}
    </>
  )
}
