import { useId, useState } from 'react'
import { ApiProblem } from '@orbit/api/problem'
import { Modal } from '@/components/common/Modal'
import { Button } from '@/components/ui/button'
import { DialogClose, DialogFooter } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useRequestEmailChange } from '@/features/auth/api'
import { CurrentPassword } from '@/features/auth/components/CurrentPassword'

/** A 401 is about the password or the sign-in, a 422 about the address; the rest shows under the form. */
function errorPlace(error: unknown): 'password' | 'email' | 'form' | null {
  if (!error) return null
  if (error instanceof ApiProblem && error.status === 401) return 'password'
  if (error instanceof ApiProblem && error.status === 422) return 'email'
  return 'form'
}

/**
 * Asks for the new address (and the current password, if the account has one). The server emails a link to the new
 * address; the email changes only once it is opened, so the dialog ends on "Check your inbox".
 */
export function EmailChangeDialog({ currentEmail, hasPassword, onClose }: { currentEmail: string; hasPassword: boolean; onClose: () => void }) {
  const request = useRequestEmailChange()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [sentTo, setSentTo] = useState<string | null>(null)
  const emailId = useId()
  const place = errorPlace(request.error)
  const detail = request.error instanceof ApiProblem ? request.error.detail : 'The email could not be changed.'

  return (
    <Modal
      title={sentTo ? 'Check your inbox' : 'Change your email'}
      description={sentTo
        ? `We sent a link to ${sentTo}. Open it within 60 minutes to finish the change. Until then, you sign in with ${currentEmail}.`
        : 'We send a link to the new address. Your email changes when you open it.'}
      onClose={onClose}
      dismissible={!request.isPending}
      className="sm:max-w-120"
    >
      {sentTo ? (
        <DialogFooter>
          <DialogClose render={<Button type="button" />}>Done</DialogClose>
        </DialogFooter>
      ) : (
        <form className="grid gap-4" onSubmit={(event) => {
          event.preventDefault()
          const next = email.trim()
          request.mutate({ email: next, password: hasPassword ? password : null }, { onSuccess: () => setSentTo(next) })
        }}>
          <FieldGroup>
            <Field data-invalid={place === 'email' || undefined}>
              <FieldLabel htmlFor={emailId}>New email</FieldLabel>
              <Input id={emailId} type="email" required autoFocus autoComplete="email" aria-invalid={place === 'email' || undefined} value={email} onChange={(event) => setEmail(event.target.value)} />
              {place === 'email' ? <FieldError>{detail}</FieldError> : <FieldDescription>Now {currentEmail}.</FieldDescription>}
            </Field>
            <CurrentPassword returnTo="/profile" autoFocus={false} hasPassword={hasPassword} value={password} onChange={setPassword} error={place === 'password' ? request.error : null} fallback="The email could not be changed." />
          </FieldGroup>
          {place === 'form' ? <p role="alert" className="text-[13px] text-destructive">{detail}</p> : null}
          <DialogFooter>
            <DialogClose render={<Button type="button" variant="ghost" disabled={request.isPending} />}>Cancel</DialogClose>
            <Button type="submit" disabled={request.isPending || !email.trim() || (hasPassword && !password)}>{request.isPending ? 'Sending…' : 'Send link'}</Button>
          </DialogFooter>
        </form>
      )}
    </Modal>
  )
}
