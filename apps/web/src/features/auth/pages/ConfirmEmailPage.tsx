import { useEffect, useRef } from 'react'
import { Link } from 'react-router'
import { ApiProblem } from '@orbit/api/problem'
import { useConfirmEmailChange } from '@/features/auth/api'
import { AuthCard } from '@/features/auth/components/AuthForm'
import { AuthMessage } from '@/features/auth/components/AuthMessage'
import { useConsumedToken } from '@/features/auth/useConsumedToken'

const ERROR_TITLES: Record<string, string> = {
  invalid_email_change_token: 'This link does not work',
  email_taken: 'This email is taken',
}

/**
 * `/confirm-email?token=…`: the link that an email change sends to the new address. Opens with or without a session,
 * since the person may read the email on another device.
 */
export function ConfirmEmailPage() {
  const token = useConsumedToken()
  const confirm = useConfirmEmailChange()
  const started = useRef(false)

  useEffect(() => {
    // The token works once.
    if (started.current || !token) return
    started.current = true
    confirm.mutate(token)
  }, [confirm, token])

  if (!token) return <AuthMessage title="This link does not work" detail="The link is missing its token. Open the link from the email again." />
  if (confirm.isSuccess) {
    return (
      <AuthCard>
        <h1 className="mb-1 text-xl font-semibold">Your email is changed</h1>
        <p className="text-[13px] leading-[1.6] text-muted-foreground">You now sign in with {confirm.data}. Your other devices are signed out.</p>
        <p className="mt-4 text-[13px]"><Link to="/" className="font-medium text-foreground">Continue to Orbit</Link></p>
      </AuthCard>
    )
  }
  if (confirm.isError) {
    const problem = confirm.error instanceof ApiProblem ? confirm.error : null
    return (
      <AuthCard>
        <h1 className="mb-1 text-xl font-semibold">{(problem && ERROR_TITLES[problem.code]) ?? 'Your email is not changed'}</h1>
        <p role="alert" className="text-[13px] leading-[1.6] text-muted-foreground">{problem ? problem.detail : 'The server could not change the email. Try opening the link again.'}</p>
        <p className="mt-4 text-[13px]"><Link to="/profile" className="font-medium text-foreground">Go to your account settings</Link></p>
      </AuthCard>
    )
  }
  return <AuthMessage title="Changing your email" detail="One moment." />
}
