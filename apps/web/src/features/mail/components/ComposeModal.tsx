import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
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
      <div className="flex flex-col gap-3">
        <div>
          <Label htmlFor="mail-compose-to" className="mb-1.5 h-4 text-[13px] leading-4 text-muted-foreground">
            To
          </Label>
          <Input
            id="mail-compose-to"
            type="text"
            placeholder="name@example.com"
            value={to}
            onChange={(e) => setTo(e.target.value)}
            autoFocus
          />
        </div>
        <div>
          <Label htmlFor="mail-compose-subject" className="mb-1.5 h-4 text-[13px] leading-4 text-muted-foreground">
            Subject
          </Label>
          <Input
            id="mail-compose-subject"
            type="text"
            placeholder="Subject"
            value={subject}
            onChange={(e) => setSubject(e.target.value)}
          />
        </div>
        <div>
          <Label htmlFor="mail-compose-body" className="mb-1.5 h-4 text-[13px] leading-4 text-muted-foreground">
            Message
          </Label>
          <Textarea
            id="mail-compose-body"
            className="block field-sizing-fixed min-h-20 resize-y px-3 py-2 text-sm leading-5 text-foreground"
            placeholder="Write your message…"
            value={body}
            onChange={(e) => setBody(e.target.value)}
          />
        </div>
        <div className="mt-4 flex flex-wrap items-center justify-end gap-2 border-t border-border pt-4">
          <Button variant="ghost" onClick={onClose}>
            Discard
          </Button>
          <Button disabled={!canSend} onClick={send}>
            Send
          </Button>
        </div>
      </div>
    </Modal>
  )
}
