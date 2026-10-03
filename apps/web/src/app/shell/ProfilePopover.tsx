import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useLocation, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { Clock, Envelope, Phone, ShieldUser } from 'reicon-react'
import { statusLabel } from '@/components/common/memberStatus'
import { ProfilePopoverContext, PROFILE_TRIGGER_ATTRIBUTE, type ProfilePopoverControl } from '@/components/common/profilePopover'
import { StatusMark } from '@/components/common/StatusMark'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { Textarea } from '@/components/ui/textarea'
import { useCurrentUser } from '@/features/auth/api'
import { useChatClient } from '@/features/chat/api/chatContext'
import { useOpenDm } from '@/features/chat/api/mutations'
import { chatErrorMessage } from '@/features/chat/components/dialogs/channelLib'
import { useChatNavigation } from '@/features/chat/useChatNavigation'
import { memberProfileQueryOptions, saveMemberNote } from '@/features/profile/api'
import { localTimeLabel, phoneHref, PROFILE_LIMITS } from '@/features/profile/profileLib'
import { CustomStatusText } from '@/features/realtime/components/CustomStatusText'
import { usePresenceOf } from '@/features/realtime/presence'
import { useOwnStatus } from '@/features/realtime/useOwnStatus'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

interface ProfileTarget {
  open: boolean
  userId: string
  name: string
  anchor: HTMLElement
  instant: boolean
}

/**
 * The one profile popover of the app, and the context that every `ProfileTrigger` opens it through. It lives in the
 * shell because it shows workspace, presence and account data and starts a direct message; a trigger knows none of that.
 */
export function ProfilePopoverProvider({ children }: { children: ReactNode }) {
  const workspaceId = useWorkspace().workspace.id
  const queryClient = useQueryClient()
  const [target, setTarget] = useState<ProfileTarget | null>(null)
  // where the focus goes back to when the popover closes
  const anchor = useRef<HTMLElement | null>(null)

  // Stable for the life of a workspace: hundreds of triggers read this context.
  const control = useMemo<ProfilePopoverControl>(
    () => ({
      open(userId, element, { name, instant }) {
        anchor.current = element
        setTarget((current) => (current?.open && current.anchor === element ? { ...current, open: false } : { open: true, userId, name, anchor: element, instant }))
      },
      prefetch(userId) {
        void queryClient.prefetchQuery(memberProfileQueryOptions(workspaceId, userId))
      },
    }),
    [queryClient, workspaceId],
  )
  const close = () => setTarget((current) => (current ? { ...current, open: false } : current))

  // The popover hangs on the element that opened it. Another page, or a list that scrolled the element away, leaves
  // nothing to hang on: the popover closes.
  const { pathname } = useLocation()
  const openAnchor = target?.open ? target.anchor : null
  useEffect(() => {
    if (!openAnchor) return
    const closeIfGone = () => {
      if (!openAnchor.isConnected) setTarget((current) => (current ? { ...current, open: false } : current))
    }
    window.addEventListener('scroll', closeIfGone, { capture: true, passive: true })
    return () => {
      window.removeEventListener('scroll', closeIfGone, { capture: true })
      // the effect ends when the page changes while the popover is open
      setTarget((current) => (current?.open && current.anchor === openAnchor && !openAnchor.isConnected ? { ...current, open: false } : current))
    }
  }, [openAnchor, pathname])

  return (
    <ProfilePopoverContext value={control}>
      {children}
      <Popover
        open={target?.open ?? false}
        modal={false}
        onOpenChange={(next, details) => {
          if (next) return
          // A press on a trigger while the popover is open is for that trigger: its click moves or closes the popover.
          const trigger = `[${PROFILE_TRIGGER_ATTRIBUTE}]`
          if (details.reason === 'outside-press' && details.event.target instanceof Element && details.event.target.closest(trigger)) return
          // the focus goes to the pressed trigger before its click; Tab to a trigger is not a press and closes
          const focused = details.reason === 'focus-out' ? (details.event as FocusEvent).relatedTarget : null
          if (focused instanceof Element && focused.closest(`${trigger}:active`)) return
          close()
        }}
      >
        {target ? (
          <PopoverContent
            anchor={target.anchor}
            side="right"
            align="start"
            finalFocus={anchor}
            aria-label={`Profile of ${target.name}`}
            {...(target.instant && { 'data-instant': '' })}
            // a floating comment thread of a page closes on a press outside it, except in a popup with this mark
            // (`COMMENT_POPUP_ATTR`; the docs editor is a lazy chunk, so the shell does not import it)
            data-comment-popup=""
            className="w-[300px] max-w-[calc(100vw-16px)] gap-0 p-0"
          >
            <ProfileCard key={target.userId} userId={target.userId} name={target.name} onClose={close} />
          </PopoverContent>
        ) : null}
      </Popover>
    </ProfilePopoverContext>
  )
}

/** The current time, moving on at each full minute while the component is mounted. */
function useMinuteClock(): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>
    const schedule = () => {
      timer = setTimeout(() => {
        setNow(Date.now())
        schedule()
      }, 60_000 - (Date.now() % 60_000) + 50)
    }
    schedule()
    return () => clearTimeout(timer)
  }, [])
  return now
}

/** One line of the details list: a 16px mark, then the text. */
function DetailRow({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <li data-slot="profile-detail" className="flex min-w-0 items-center gap-2 [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-muted-foreground">
      <span className="flex size-4 shrink-0 items-center justify-center">{icon}</span>
      {children}
    </li>
  )
}

function ProfileCard({ userId, name, onClose }: { userId: string; name: string; onClose: () => void }) {
  const workspaceId = useWorkspace().workspace.id
  const navigate = useNavigate()
  const me = useCurrentUser().data
  const members = useMembers(workspaceId)
  const member = members.data?.find((candidate) => candidate.id === userId)
  const own = me?.id === userId
  const profileQuery = useQuery({ ...memberProfileQueryOptions(workspaceId, userId), enabled: Boolean(member) })
  const others = usePresenceOf(userId)
  const mine = useOwnStatus()
  const now = useMinuteClock()

  // the members list is in the cache of every signed-in page; until it is, the popover has nothing to say
  if (members.isPending) return <div data-slot="profile-card" className="h-24" />
  if (!member || profileQuery.data === null) {
    return (
      <div data-slot="profile-card" className="flex flex-col gap-0.5 p-4">
        <p className="text-base font-semibold wrap-anywhere">{member?.name ?? name}</p>
        <p className="text-[13px] text-muted-foreground">No longer in this workspace</p>
      </div>
    )
  }

  // the user's own profile and status come from the session: there an invisible user is not offline
  const profile = own ? me : profileQuery.data
  const status = own ? mine.presence : others.status
  const custom = own ? mine : others
  const localTime = localTimeLabel(profile?.timezone, now)
  const copyEmail = () => void navigator.clipboard.writeText(member.email).then(() => toast('Email copied'), () => toast.error('Could not copy to the clipboard.'))

  return (
    <div data-slot="profile-card" className="flex flex-col gap-3 p-4">
      <div className="flex items-start gap-3">
        <UserAvatar user={member} size={64} status={status} className="shrink-0" />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5 pt-1">
          <p className="text-base leading-snug font-semibold wrap-anywhere">
            {member.name}
            {profile?.pronouns ? <span className="ml-1.5 text-[13px] font-normal text-muted-foreground">{profile.pronouns}</span> : null}
          </p>
          {profile?.title ? <p className="text-[13px] text-muted-foreground wrap-anywhere">{profile.title}</p> : null}
          {custom.emoji !== null || custom.text !== null ? (
            <p data-slot="profile-status" className="mt-1 text-[13px] wrap-anywhere">
              <CustomStatusText emoji={custom.emoji} text={custom.text} />
            </p>
          ) : null}
        </div>
      </div>

      <ul className="flex flex-col gap-1.5 text-[13px]">
        <DetailRow icon={<StatusMark status={status} size={10} />}>{statusLabel(status)}</DetailRow>
        {localTime ? <DetailRow icon={<Clock aria-hidden="true" />}>{localTime}</DetailRow> : null}
        <DetailRow icon={<Envelope aria-hidden="true" />}>
          <button
            type="button"
            className="min-w-0 cursor-pointer truncate rounded-sm text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50 hover-fine:hover:underline"
            aria-label={`Copy email address ${member.email}`}
            onClick={copyEmail}
          >
            {member.email}
          </button>
        </DetailRow>
        {profile?.phone ? (
          <DetailRow icon={<Phone aria-hidden="true" />}>
            <a
              href={phoneHref(profile.phone)}
              className="min-w-0 truncate rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50 hover-fine:hover:underline"
              aria-label={`Call ${profile.phone}`}
            >
              {profile.phone}
            </a>
          </DetailRow>
        ) : null}
        <DetailRow icon={<ShieldUser aria-hidden="true" />}>{member.role}</DetailRow>
      </ul>

      {profile?.bio ? <p data-slot="profile-bio" className="max-h-32 overflow-y-auto text-[13px] wrap-anywhere whitespace-pre-wrap text-foreground/85">{profile.bio}</p> : null}

      {own ? (
        <Button
          variant="outline"
          className="w-full"
          onClick={() => {
            onClose()
            navigate('/profile')
          }}
        >
          Edit profile
        </Button>
      ) : (
        <MessageAction userId={userId} name={member.name} onDone={onClose} />
      )}

      {!own && profileQuery.data ? <ProfileNote workspaceId={workspaceId} userId={userId} name={member.name} saved={profileQuery.data.note ?? ''} /> : null}
    </div>
  )
}

/** "Message": opens the direct message with the member (the server makes it when there is none), then closes the popover. */
function MessageAction({ userId, name, onDone }: { userId: string; name: string; onDone: () => void }) {
  const client = useChatClient()
  const openDm = useOpenDm()
  const { openConversation } = useChatNavigation()
  if (!client) return null
  return (
    <Button
      className="w-full"
      disabled={openDm.isPending}
      onClick={() =>
        openDm.mutate([userId], {
          onSuccess: (dm) => {
            onDone()
            openConversation(dm.id)
          },
          onError: (error) => toast.error(chatErrorMessage(error, `Could not open the conversation with ${name}. Try again.`)),
        })
      }
    >
      Message
    </Button>
  )
}

/**
 * The private note about a member. It saves when the field loses the focus and when the popover goes away (closed, or
 * moved to somebody else). Not `data-realtime-safe`: this is a short draft, and the pause of workspace refreshes while
 * it has the focus is what keeps a refetch of the profile from racing the text.
 */
function ProfileNote({ workspaceId, userId, name, saved }: { workspaceId: string; userId: string; name: string; saved: string }) {
  const queryClient = useQueryClient()
  // `null` until the user types: the field then shows the saved note
  const [draft, setDraft] = useState<string | null>(null)
  const latest = useRef<string | null>(null)

  const save = () => {
    if (latest.current !== null) void saveMemberNote(queryClient, workspaceId, userId, latest.current)
  }
  // a popover that closes takes the field away without a blur event in some browsers
  useEffect(
    () => () => {
      if (latest.current !== null) void saveMemberNote(queryClient, workspaceId, userId, latest.current)
    },
    [queryClient, workspaceId, userId],
  )

  return (
    <Textarea
      data-slot="profile-note"
      aria-label={`Private note about ${name}`}
      placeholder="Add a note. Only you can see it."
      maxLength={PROFILE_LIMITS.note}
      rows={2}
      value={draft ?? saved}
      className="max-h-32 min-h-14 resize-none border-0 bg-secondary/40 text-[13px] md:text-[13px] dark:bg-secondary/40"
      onChange={(event) => {
        latest.current = event.target.value
        setDraft(event.target.value)
      }}
      onBlur={save}
    />
  )
}
