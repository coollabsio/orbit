import { useId, useState, type FormEvent } from 'react'
import { toast } from 'sonner'
import type { ChatReportReason } from '@orbit/api/generated/types.gen'
import { InfoTip } from '@/components/common/InfoTip'
import { Modal } from '@/components/common/Modal'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldLabel, FieldLegend, FieldSet } from '@/components/ui/field'
import { Label } from '@/components/ui/label'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Textarea } from '@/components/ui/textarea'
import { useChatContext } from '../../api/chatContext'
import { REPORT_NOTE_MAX, REPORT_REASONS, useReportMessage } from '../../api/moderation'
import type { Message } from '../../api/types'

/** Reports a message to the workspace owners and admins: a reason and an optional note. Mounted while open. */
export function ReportDialog({ message, onClose }: { message: Message; onClose: () => void }) {
  const { workspaceId } = useChatContext()
  const report = useReportMessage(workspaceId)
  const [reason, setReason] = useState<ChatReportReason | null>(null)
  const [note, setNote] = useState('')
  const noteId = useId()

  function submit(event: FormEvent) {
    event.preventDefault()
    if (!reason) return
    report.mutate(
      { messageId: message.id, reason, note },
      {
        onSuccess: () => {
          toast.success('Thank you. Workspace owners and admins will look at your report.')
          onClose()
        },
        onError: () => toast.error('Could not send the report. Try again.'),
      },
    )
  }

  return (
    <Modal title="Report message" onClose={onClose} className="sm:max-w-md" dismissible={!report.isPending}>
      <form className="grid gap-4" onSubmit={submit}>
        <FieldSet>
          <FieldLegend variant="label">Reason</FieldLegend>
          <RadioGroup value={reason} onValueChange={(value) => setReason(value as ChatReportReason)}>
            {Object.entries(REPORT_REASONS).map(([value, label]) => (
              <Label key={value} className="font-normal">
                <RadioGroupItem value={value} />
                {label}
              </Label>
            ))}
          </RadioGroup>
        </FieldSet>
        <Field>
          <FieldLabel htmlFor={noteId} className="items-center gap-1.5">
            Note <InfoTip text="Optional. Tell the workspace owners and admins more about the problem." />
          </FieldLabel>
          <Textarea id={noteId} rows={3} maxLength={REPORT_NOTE_MAX} value={note} onChange={(event) => setNote(event.target.value)} />
        </Field>
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" disabled={report.isPending} />}>Cancel</DialogClose>
          <Button type="submit" disabled={!reason || report.isPending}>
            Report
          </Button>
        </DialogFooter>
      </form>
    </Modal>
  )
}
