import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router'
import { createApiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { Button } from '@/components/ui/button'
import { ApiProblem } from '@/api/problem'
import { currentUserQueryOptions, isTwoFactorChallenge, useLogin, useLoginSecondFactor, useLogout } from '@/features/auth/api'
import { AuthForm, AuthInput } from '@/features/auth/components/AuthForm'
import { AuthMessage } from '@/features/auth/components/AuthMessage'
import { useConsumedToken } from '@/features/auth/useConsumedToken'
import { useAcceptInvitation, useInvitationPreview } from '@/features/workspaces/api'

// Anonymous sessions are expected here; do not trigger the global login redirect.
const invitationSessionClient = createApiClient()

export function AcceptInvitationPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const token = useConsumedToken()
  const invitation = useInvitationPreview(token)
  const user = useQuery({ ...currentUserQueryOptions(invitationSessionClient), enabled: !!token, retry: false })
  const accept = useAcceptInvitation()
  const login = useLogin()
  const secondFactor = useLoginSecondFactor()
  const logout = useLogout()
  // Set while an account with two-factor sign-in waits for its code.
  const [twoFactorToken, setTwoFactorToken] = useState<string | null>(null)
  const [code, setCode] = useState('')
  const [signIn, setSignIn] = useState(false)
  const [displayName, setDisplayName] = useState('')
  const [password, setPassword] = useState('')
  if (!token) return <AuthMessage title="Invitation unavailable" detail="This invitation link is missing its token." />
  if (invitation.isError) return <AuthMessage title="Invitation unavailable" detail="This invitation could not be loaded. Try reopening the link, or ask the inviter for a new invitation." />
  if (user.isError) return <AuthMessage title="Orbit is unavailable" detail="The server could not verify this session. Try reopening the invitation." />
  if (!invitation.data || user.isPending) return <AuthMessage title="Loading invitation" detail="Checking the invitation and your session." />

  const { email, workspace_name: workspaceName } = invitation.data
  const mismatch = !!user.data && user.data.email.trim().toLowerCase() !== email.trim().toLowerCase()
  const pending = accept.isPending || login.isPending || secondFactor.isPending || logout.isPending
  return (
    <AuthForm
      title={`Join ${workspaceName}`}
      error={accept.error || login.error || secondFactor.error || logout.error}
      pending={pending}
      submitLabel={mismatch ? 'Switch account' : !user.data && signIn ? 'Sign in and accept invitation' : 'Accept invitation'}
      onSubmit={async () => {
        if (mismatch) {
          await logout.mutateAsync()
          queryClient.setQueryData(queryKeys.currentUser, null)
          setPassword('')
          setSignIn(true)
          return
        }
        if (!user.data && signIn && twoFactorToken) {
          await secondFactor.mutateAsync({ two_factor_token: twoFactorToken, code }).catch((error: unknown) => {
            // The sign-in expired: start again from the password.
            if (error instanceof ApiProblem && error.code === 'invalid_two_factor_token') setTwoFactorToken(null)
            throw error
          })
        } else if (!user.data && signIn) {
          const response = await login.mutateAsync({ email, password })
          if (isTwoFactorChallenge(response)) {
            setCode('')
            setTwoFactorToken(response.two_factor_token)
            return
          }
        }
        const accepted = await accept.mutateAsync(!user.data && !signIn
          ? { token, email, display_name: displayName, password }
          : { token })
        navigate(`/?workspace=${encodeURIComponent(accepted.workspace_id)}`, { replace: true })
      }}
      footer={!user.data ? <Button variant="outline" type="button" disabled={pending} onClick={() => {
        setSignIn((value) => !value)
        setPassword('')
        setTwoFactorToken(null)
        login.reset()
        secondFactor.reset()
        accept.reset()
      }}>{signIn ? 'Create an account instead' : 'Sign in instead'}</Button> : undefined}
    >
      <AuthInput label="Email" type="email" value={email} required={false} />
      {mismatch ? <p className="text-[13px]">You are signed in as {user.data!.email}. Switch accounts to accept this invitation as {email}.</p> : (
        <>
          <p className="text-[13px]">{user.data ? 'Accept to join this workspace with your account.' : signIn ? 'Sign in with your existing account to join this workspace.' : 'Choose your name and password to create your account.'} To use another email, ask the inviter for a new invitation.</p>
          {!user.data && !signIn ? <AuthInput label="Your name" value={displayName} onChange={setDisplayName} /> : null}
          {!user.data && twoFactorToken ? <AuthInput label="Authentication code" value={code} onChange={setCode} /> : null}
          {!user.data && !twoFactorToken ? <AuthInput label="Password" type="password" value={password} onChange={setPassword} /> : null}
        </>
      )}
    </AuthForm>
  )
}
