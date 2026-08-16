import { useState } from 'react'
import { useNavigate } from 'react-router'
import { Modal } from '../../../components/ui/Modal'
import { composeMail } from '../../../mock/actions'

interface ComposeModalProps {
  onClose: () => void
}

export function ComposeModal({ onClose }: ComposeModalProps) {
  const navigate = useNavigate()
  const [to, setTo] = useState('')
  const [subject, setSubject] = useState('')
  const [body, setBody] = useState('')
  const canSend = to.trim() !== '' && body.trim() !== ''

  const send = () => {
    if (!canSend) return
    composeMail({ to: to.trim(), subject: subject.trim(), body: body.trim() })
    onClose()
    navigate('/mail?folder=f_sent')
  }

  return (
    <Modal title="New message" onClose={onClose}>
      <div className="mail-compose">
        <div>
          <label className="field-label" htmlFor="mail-compose-to">
            To
          </label>
          <input
            id="mail-compose-to"
            className="input"
            type="text"
            placeholder="name@example.com"
            value={to}
            onChange={(e) => setTo(e.target.value)}
          />
        </div>
        <div>
          <label className="field-label" htmlFor="mail-compose-subject">
            Subject
          </label>
          <input
            id="mail-compose-subject"
            className="input"
            type="text"
            placeholder="Subject"
            value={subject}
            onChange={(e) => setSubject(e.target.value)}
          />
        </div>
        <div>
          <label className="field-label" htmlFor="mail-compose-body">
            Message
          </label>
          <textarea
            id="mail-compose-body"
            className="input"
            placeholder="Write your message…"
            value={body}
            onChange={(e) => setBody(e.target.value)}
          />
        </div>
        <div className="mail-compose-footer">
          <span className="spacer" />
          <button className="button button-ghost" onClick={onClose}>
            Discard
          </button>
          <button className="button button-primary" disabled={!canSend} onClick={send}>
            Send
          </button>
        </div>
      </div>
    </Modal>
  )
}
