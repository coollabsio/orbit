import { useId } from 'react'
import { ApiProblem } from '@orbit/api/problem'
import { Field, FieldError, FieldLabel } from '@/components/ui/field'
import { PasswordInput } from '@/components/ui/password-input'
import { ConfirmWithProvider } from '@/features/auth/components/ConfirmWithProvider'

/**
 * The current password, asked again before a change to the account. An account without one has no field: when its
 * sign-in is too old, the server asks for a fresh provider sign-in instead, which comes back to `returnTo`.
 */
export function CurrentPassword({ hasPassword, value, onChange, error, fallback, returnTo, autoFocus = true }: { hasPassword: boolean; value: string; onChange: (value: string) => void; error: unknown; fallback: string; returnTo: string; autoFocus?: boolean }) {
  const id = useId()
  const message = error ? (error instanceof ApiProblem ? error.detail : fallback) : null
  if (!hasPassword) {
    if (error instanceof ApiProblem && error.code === 'reauthentication_required') return <ConfirmWithProvider returnTo={returnTo} />
    return message ? <p role="alert" className="text-[13px] text-destructive">{message}</p> : null
  }
  return (
    <Field data-invalid={message ? true : undefined}>
      <FieldLabel htmlFor={id}>Current password</FieldLabel>
      <PasswordInput id={id} required autoFocus={autoFocus} autoComplete="current-password" aria-invalid={message ? true : undefined} value={value} onChange={(event) => onChange(event.target.value)} />
      {message ? <FieldError>{message}</FieldError> : null}
    </Field>
  )
}
