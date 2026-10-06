import { useRef, useState, type FormEvent } from 'react'
import { Sticker } from 'reicon-react'
import type { CustomStickerRecord } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { confirmAction } from '@/components/common/confirmAction'
import { EmptyState } from '@/components/common/EmptyState'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { FieldGrid, RequiredMark, SettingsRow } from '@/features/settings/components/SettingsParts'
import { useMembers } from '@/features/workspaces/api'
import { useCreateCustomSticker, useCustomStickers, useDeleteCustomSticker } from '@/features/workspaces/customStickers'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { STICKER_IMAGE_TYPES, STICKER_NAME_MAX, stickerNameFromFile, stickerProblem } from '@/lib/customSticker'
import { shortDate } from '@/lib/format'

/** What the server can answer to a new sticker or a delete, in words for the page. */
const SERVER_PROBLEMS: Record<string, string> = {
  chat_forbidden: 'Only workspace owners and administrators can manage stickers.',
  sticker_too_large: 'The image is too large. The limit is 512 KB.',
  invalid_sticker: 'The file is not a PNG, JPEG, WebP or GIF image.',
  validation_failed: 'A name has 2 to 30 characters.',
  sticker_name_taken: 'A sticker with this name exists already. Choose another name.',
  sticker_limit_reached: 'The workspace has 100 stickers, which is the limit. Delete one first.',
}

/** The reason in words: ours for a known code, else what the server said (with the status), else that it did not answer. */
function serverProblem(error: unknown, fallback: string): string {
  if (!(error instanceof ApiProblem)) return `${fallback} Orbit did not answer. Check your connection and try again.`
  return SERVER_PROBLEMS[error.code] ?? `${fallback} ${error.detail} (${error.status} ${error.code})`
}

/** The stickers of the workspace: add one from an image, delete one. Owners and administrators only (`chat.manage`). */
export function StickersPage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('chat.manage')
  const stickers = useCustomStickers(workspace.id, canManage)
  const members = useMembers(canManage ? workspace.id : null)
  const createSticker = useCreateCustomSticker(workspace.id)
  const deleteSticker = useDeleteCustomSticker(workspace.id)
  const fileInput = useRef<HTMLInputElement>(null)
  const [file, setFile] = useState<File | null>(null)
  const [name, setName] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  /** From the click to the answer: a second click must not send a second sticker. */
  const [submitting, setSubmitting] = useState(false)

  if (!canManage) return <SettingsCard title="Stickers"><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage stickers.</p></SettingsCard>

  const memberNames = new Map<string, string>(members.data?.map((member) => [member.id, member.name]))

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (!file || submitting) return
    const refused = stickerProblem(file, name, stickers.data?.map((entry) => entry.name) ?? [])
    setProblem(refused)
    if (refused) return
    setSubmitting(true)
    try {
      await createSticker.mutateAsync({ name: name.trim(), file })
      setFile(null)
      setName('')
      if (fileInput.current) fileInput.current.value = ''
    } catch (error) {
      setProblem(serverProblem(error, 'The sticker could not be added.'))
    } finally {
      setSubmitting(false)
    }
  }

  async function remove(entry: CustomStickerRecord) {
    if (!await confirmAction({ title: `Delete ${entry.name}?`, description: 'Messages that were sent with it say that the sticker was deleted. You cannot undo this.', confirmLabel: 'Delete', danger: true })) return
    deleteSticker.mutate(entry)
  }

  return <>
    <SettingsCard title="Add sticker" description="Stickers are for everyone in this workspace: a large image that is sent as a chat message.">
      <form className="flex flex-col gap-4" onSubmit={(event) => void submit(event)}>
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="sticker-file">Image <RequiredMark /></FieldLabel>
            <Input
              ref={fileInput}
              id="sticker-file"
              type="file"
              accept={STICKER_IMAGE_TYPES.join(',')}
              onChange={(event) => {
                const chosen = event.target.files?.[0] ?? null
                setFile(chosen)
                setProblem(null)
                if (chosen) setName(stickerNameFromFile(chosen.name))
              }}
            />
            <FieldDescription>PNG, JPEG, WebP or GIF, at most 512 KB.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="sticker-name">Name <RequiredMark /></FieldLabel>
            <Input id="sticker-name" value={name} maxLength={STICKER_NAME_MAX} placeholder="Party Parrot" onChange={(event) => { setName(event.target.value); setProblem(null) }} />
            <FieldDescription>2 to 30 characters. People find the sticker by this name.</FieldDescription>
          </Field>
        </FieldGrid>
        <div className="flex flex-wrap items-center gap-3">
          <Button type="submit" disabled={!file || !name.trim() || submitting}>Add sticker</Button>
          {problem ? <p className="text-xs text-destructive" role="alert">{problem}</p> : null}
        </div>
      </form>
    </SettingsCard>
    {stickers.data?.length === 0 ? (
      <EmptyState icon={Sticker} title="No stickers yet" description="Add an image above. Everyone in the workspace can then send it in chat." />
    ) : (
      <SettingsCard title="Stickers" description={stickers.data ? `${stickers.data.length} of 100 in this workspace.` : undefined} flush>
        <div className="flex flex-col divide-y">
          {stickers.isPending ? <SettingsRow>Loading stickers…</SettingsRow> : null}
          {stickers.isError ? <SettingsRow role="alert">Stickers could not be loaded.</SettingsRow> : null}
          {stickers.data?.map((entry) => (
            <SettingsRow className="flex-nowrap" key={entry.id}>
              <img src={entry.url} alt="" draggable={false} loading="lazy" className="size-16 shrink-0 object-contain" />
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="truncate text-[13px] font-medium text-foreground">{entry.name}</span>
                <span className="truncate text-xs text-muted-foreground/70">Added by {memberNames.get(entry.created_by) ?? 'a former member'} · {shortDate(entry.created_at)}</span>
              </div>
              <Button variant="ghost" type="button" aria-label={`Delete ${entry.name}`} disabled={deleteSticker.isPending} onClick={() => void remove(entry)}>Delete</Button>
            </SettingsRow>
          ))}
          {deleteSticker.isError ? <SettingsRow className="text-xs text-destructive" role="alert">{serverProblem(deleteSticker.error, 'The sticker could not be deleted.')}</SettingsRow> : null}
        </div>
      </SettingsCard>
    )}
  </>
}
