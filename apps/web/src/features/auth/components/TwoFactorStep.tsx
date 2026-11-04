import { useId, useState } from 'react'
import { ApiProblem } from '@orbit/api/problem'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useLoginSecondFactor } from '@/features/auth/api'
import { AuthForm } from '@/features/auth/components/AuthForm'

/**
 * The code step after a sign-in the account guards with two-factor. `onBack` gets the reason when the challenge
 * expired or was used up, and `null` when the person pressed Back.
 */
export function TwoFactorStep({ token, onSignedIn, onBack }: { token: string; onSignedIn: () => void; onBack: (error: Error | null) => void }) {
  const secondFactor = useLoginSecondFactor()
  const [code, setCode] = useState('')
  const codeId = useId()
  return (
    <AuthForm title="Two-factor sign-in" error={secondFactor.error} pending={secondFactor.isPending} submitLabel="Verify" onSubmit={async () => {
      try {
        await secondFactor.mutateAsync({ two_factor_token: token, code: code.trim() })
      } catch (error) {
        if (error instanceof ApiProblem && error.code === 'invalid_two_factor_token') onBack(error)
        throw error
      }
      onSignedIn()
    }} footer={<Button type="button" variant="link" className="h-auto p-0" onClick={() => onBack(null)}>Back</Button>}>
      <Field className="gap-1.5">
        <FieldLabel htmlFor={codeId}>Authentication code</FieldLabel>
        <Input id={codeId} required autoFocus autoComplete="one-time-code" spellCheck={false} aria-describedby={`${codeId}-hint`} value={code} onChange={(event) => setCode(event.target.value)} />
        <FieldDescription id={`${codeId}-hint`}>Enter the 6-digit code from your authenticator app, or a recovery code.</FieldDescription>
      </Field>
    </AuthForm>
  )
}
