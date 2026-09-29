import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'
import { Modal } from '@/components/common/Modal'
import { composeMail } from '@/mock/actions'

interface ComposeModalProps {
  onClose: () => void
  initial?: { to?: string; subject?: string; body?: string }
}

export function ComposeModal({ onClose, initial }: ComposeModalProps) {
  const navigate = useNavigate()
  const [to, setTo] = useState(initial?.to ?? '')
  const [subject, setSubject] = useState(initial?.subject ?? '')
  const [body, setBody] = useState(initial?.body ?? '')
  const canSend = to.trim() !== '' && body.trim() !== ''

  const send = () => {
    if (!canSend) return
    composeMail({ to: to.trim(), subject: subject.trim(), body: body.trim() })
    onClose()
    navigate('/mail?folder=f_sent')
  }

  return (
    <Modal title="New message" onClose={onClose}>
      <div className="flex flex-col gap-4">
        <Field>
          <FieldLabel htmlFor="mail-compose-to">
            To
          </FieldLabel>
          <Input
            id="mail-compose-to"
            type="text"
            placeholder="name@example.com"
            value={to}
            onChange={(e) => setTo(e.target.value)}
            autoFocus
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="mail-compose-subject">
            Subject
          </FieldLabel>
          <Input
            id="mail-compose-subject"
            type="text"
            placeholder="Subject"
            value={subject}
            onChange={(e) => setSubject(e.target.value)}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="mail-compose-body">
            Message
          </FieldLabel>
          <Textarea
            id="mail-compose-body"
            className="field-sizing-fixed min-h-20 resize-y"
            placeholder="Write your message…"
            value={body}
            onChange={(e) => setBody(e.target.value)}
          />
        </Field>
        <DialogFooter>
          <DialogClose render={<Button variant="ghost" />}>
            Discard
          </DialogClose>
          <Button disabled={!canSend} onClick={send}>
            Send
          </Button>
        </DialogFooter>
      </div>
    </Modal>
  )
}
