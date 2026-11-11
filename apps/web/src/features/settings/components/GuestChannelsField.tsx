import { Hashtag } from 'reicon-react'
import { InfoTip } from '@/components/common/InfoTip'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Field, FieldLabel } from '@/components/ui/field'
import { useConversations } from '@/features/chat/api/queries'
import { RequiredMark } from './SettingsParts'

/** The private channels a guest joins. Only the open private channels the admin is in can be chosen. */
export function GuestChannelsField({ value, onChange }: { value: readonly string[]; onChange: (ids: string[]) => void }) {
  const conversations = useConversations()
  const channels = (conversations.data ?? [])
    .filter((conversation) => conversation.kind === 'private' && !conversation.archived)
    .sort((a, b) => a.name.localeCompare(b.name))
  const toggle = (id: string, checked: boolean) => onChange(checked ? [...value, id] : value.filter((item) => item !== id))
  return (
    <Field className="mt-4">
      <FieldLabel id="guest-channels-label" className="items-center gap-1.5">
        Channels <RequiredMark /> <InfoTip text="A guest sees only these private channels and the people in them." />
      </FieldLabel>
      {conversations.isPending ? (
        <p role="status" className="text-xs text-muted-foreground">Loading channels…</p>
      ) : !conversations.data ? (
        <div role="alert" className="flex items-center gap-2 text-xs text-destructive">
          Channels could not be loaded.
          <Button type="button" size="xs" variant="outline" disabled={conversations.isFetching} onClick={() => void conversations.refetch()}>Retry</Button>
        </div>
      ) : channels.length === 0 ? (
        <p className="text-xs text-muted-foreground">You are in no private channel. Make one in Chat first.</p>
      ) : (
        <ul aria-labelledby="guest-channels-label" className="flex max-h-48 flex-col gap-px overflow-y-auto rounded-md border p-1">
          {channels.map((channel) => (
            <li key={channel.id}>
              <label className="flex h-8 cursor-pointer items-center gap-2 rounded-sm px-2 text-[0.8125rem] hover:bg-muted">
                <Checkbox checked={value.includes(channel.id)} onCheckedChange={(checked) => toggle(channel.id, checked)} />
                <Hashtag className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
                <span className="truncate">{channel.name}</span>
              </label>
            </li>
          ))}
        </ul>
      )}
    </Field>
  )
}
