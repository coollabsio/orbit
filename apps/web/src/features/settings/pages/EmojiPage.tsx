import { useRef, useState, type FormEvent } from 'react'
import { SmileCircle } from 'reicon-react'
import type { CustomEmojiRecord } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { EmptyState } from '@/components/common/EmptyState'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { emojiNamed, loadEmojiIndex } from '@/features/chat/lib/emojiSearch'
import { FieldGrid, RequiredMark, RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { useMembers } from '@/features/workspaces/api'
import { useCreateCustomEmoji, useCustomEmoji, useDeleteCustomEmoji } from '@/features/workspaces/customEmoji'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { EMOJI_IMAGE_TYPES, emojiNameFromFile, emojiProblem } from '@/lib/customEmoji'
import { shortDate } from '@/lib/format'
import { EMOJI_SHORTCODES } from '@/lib/markdown'

/** What the server can answer to a new emoji or a delete, in words for the page. */
const SERVER_PROBLEMS: Record<string, string> = {
  chat_forbidden: 'Only workspace owners and administrators can manage custom emoji.',
  emoji_too_large: 'The image is too large. The limit is 256 KB.',
  invalid_emoji: 'The file is not a PNG, JPEG, WebP or GIF image.',
  validation_failed: 'A name has 2 to 32 characters: lower case letters, digits and underscores.',
  emoji_name_taken: 'An emoji with this name exists already. Choose another name.',
  emoji_limit_reached: 'The workspace has 200 custom emoji, which is the limit. Delete one first.',
}

/** The reason in words: ours for a known code, else what the server said (with the status), else that it did not answer. */
function serverProblem(error: unknown, fallback: string): string {
  if (!(error instanceof ApiProblem)) return `${fallback} Orbit did not answer. Check your connection and try again.`
  return SERVER_PROBLEMS[error.code] ?? `${fallback} ${error.detail} (${error.status} ${error.code})`
}

/** The custom emoji of the workspace: add one from an image, delete one. Owners and administrators only (`chat.manage`). */
export function EmojiPage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('chat.manage')
  const emoji = useCustomEmoji(workspace.id, canManage)
  const members = useMembers(canManage ? workspace.id : null)
  const createEmoji = useCreateCustomEmoji(workspace.id)
  const deleteEmoji = useDeleteCustomEmoji(workspace.id)
  const fileInput = useRef<HTMLInputElement>(null)
  const [file, setFile] = useState<File | null>(null)
  const [name, setName] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  /** From the click to the answer: the emoji data loads before the request, and a second click must not send a second one. */
  const [submitting, setSubmitting] = useState(false)

  if (!canManage) return <SettingsCard title="Emoji"><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage custom emoji.</p></SettingsCard>

  const memberNames = new Map<string, string>(members.data?.map((member) => [member.id, member.name]))

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (!file || submitting) return
    setSubmitting(true)
    // A standard emoji keeps its name (`:joy:` becomes the character as it is typed); without the data only the server's rules apply.
    const standard = await loadEmojiIndex().catch(() => [])
    const taken = (candidate: string) =>
      Boolean(emoji.data?.some((entry) => entry.name === candidate)) || Object.hasOwn(EMOJI_SHORTCODES, candidate) || emojiNamed(standard, candidate) !== null
    const refused = emojiProblem(file, name, taken)
    setProblem(refused)
    try {
      if (refused) return
      await createEmoji.mutateAsync({ name, file })
      setFile(null)
      setName('')
      if (fileInput.current) fileInput.current.value = ''
    } catch (error) {
      setProblem(serverProblem(error, 'The emoji could not be added.'))
    } finally {
      setSubmitting(false)
    }
  }

  async function remove(entry: CustomEmojiRecord) {
    if (!await confirmAction({ title: `Delete :${entry.name}:?`, description: 'Messages and reactions that use it show its name as text. You cannot undo this.', confirmLabel: 'Delete', danger: true })) return
    deleteEmoji.mutate(entry)
  }

  return <>
    <SettingsCard title="Add emoji" description="Custom emoji are for everyone in this workspace: in chat messages and as reactions.">
      <form className="flex flex-col gap-4" onSubmit={(event) => void submit(event)}>
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="emoji-file">Image <RequiredMark /></FieldLabel>
            <Input
              ref={fileInput}
              id="emoji-file"
              type="file"
              accept={EMOJI_IMAGE_TYPES.join(',')}
              onChange={(event) => {
                const chosen = event.target.files?.[0] ?? null
                setFile(chosen)
                setProblem(null)
                if (chosen) setName(emojiNameFromFile(chosen.name))
              }}
            />
            <FieldDescription>PNG, JPEG, WebP or GIF, at most 256 KB.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="emoji-name">Name <RequiredMark /></FieldLabel>
            <Input id="emoji-name" value={name} maxLength={32} placeholder="party_parrot" onChange={(event) => { setName(event.target.value.toLowerCase()); setProblem(null) }} />
            <FieldDescription>Lower case letters, digits and underscores. In a message, type :{name || 'name'}:</FieldDescription>
          </Field>
        </FieldGrid>
        <div className="flex flex-wrap items-center gap-3">
          <Button type="submit" disabled={!file || !name || submitting}>Add emoji</Button>
          {problem ? <p className="text-xs text-destructive" role="alert">{problem}</p> : null}
        </div>
      </form>
    </SettingsCard>
    {emoji.data?.length === 0 ? (
      <EmptyState icon={SmileCircle} title="No custom emoji yet" description="Add an image above. Everyone in the workspace can then use it in chat." />
    ) : (
      <SettingsCard title="Emoji" description={emoji.data ? `${emoji.data.length} of 200 in this workspace.` : undefined} flush>
        <div className="flex flex-col divide-y">
          {emoji.isPending ? <SettingsRow>Loading emoji…</SettingsRow> : null}
          {emoji.isError ? <SettingsRow role="alert">Emoji could not be loaded.</SettingsRow> : null}
          {emoji.data?.map((entry) => (
            <SettingsRow className="flex-nowrap" key={entry.id}>
              <RowIcon><img src={entry.url} alt="" draggable={false} className="size-6 object-contain" /></RowIcon>
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-[13px] font-medium text-foreground">:{entry.name}:</span>
                <span className="truncate text-xs text-muted-foreground/70">Added by {memberNames.get(entry.created_by) ?? 'a former member'} · {shortDate(entry.created_at)}</span>
              </div>
              <Button variant="ghost" type="button" aria-label={`Delete :${entry.name}:`} disabled={deleteEmoji.isPending} onClick={() => void remove(entry)}>Delete</Button>
            </SettingsRow>
          ))}
          {deleteEmoji.isError ? <SettingsRow className="text-xs text-destructive" role="alert">{serverProblem(deleteEmoji.error, 'The emoji could not be deleted.')}</SettingsRow> : null}
        </div>
      </SettingsCard>
    )}
  </>
}
