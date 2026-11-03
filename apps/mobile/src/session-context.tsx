import { useQueryClient } from '@tanstack/react-query'
import { createContext, type ReactNode, use, useEffect, useRef, useState } from 'react'

import { createClient, currentUser, type OrbitClient, signOut as endSession, type User } from './data/orbit'
import { launchErrorEndsSession } from './data/session-launch'
import { clearSession, loadSession, saveSession, type StoredSession } from './session'

export type SessionState =
  | { status: 'loading' }
  | { status: 'signed_out'; error?: string }
  /** The saved session could not be checked (offline, a 5xx): it is kept, and `retry` checks it again. */
  | { status: 'offline'; session: StoredSession; error: string }
  | { status: 'signed_in'; session: StoredSession; user: User; client: OrbitClient }

interface SessionContextValue {
  state: SessionState
  signIn: (session: StoredSession, user: User) => Promise<void>
  signOut: () => Promise<void>
  /** Checks the saved session again after `offline`. */
  retry: () => Promise<void>
  /** The signed-in user after a profile change. */
  setUser: (user: User) => void
}

const SessionContext = createContext<SessionContextValue | null>(null)

export const errorMessage = (error: unknown) => (error instanceof Error ? error.message : String(error))

export function SessionProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const [state, setState] = useState<SessionState>({ status: 'loading' })
  /** The session the current client belongs to: a late 401 from a session that already ended must not end the next one. */
  const current = useRef<StoredSession | null>(null)

  const end = async (error?: string) => {
    current.current = null
    await clearSession()
    queryClient.clear()
    setState({ status: 'signed_out', error })
  }

  /** One client per session. A 401 means the session was revoked or expired: back to sign-in. */
  const open = (session: StoredSession) => {
    current.current = session
    return createClient(
      session.server,
      () => session.token,
      () => {
        if (current.current === session) void end('Your session ended. Sign in again.')
      },
      // a write the workspace's subscription refused: fetch the billing state again, so the banner shows at once
      () => void queryClient.invalidateQueries({ queryKey: ['billing'] }),
    )
  }

  /** Opens the saved session if the server still accepts it. A 401 already signed out through the client. */
  const resume = async (session: StoredSession) => {
    const client = open(session)
    try {
      setState({ status: 'signed_in', session, user: await currentUser(client), client })
    } catch (error) {
      if (!launchErrorEndsSession(error) && current.current === session) setState({ status: 'offline', session, error: errorMessage(error) })
    }
  }

  useEffect(() => {
    void (async () => {
      const session = await loadSession()
      if (!session) return setState({ status: 'signed_out' })
      await resume(session)
    })()
  }, [])

  const retry = async () => {
    if (state.status === 'offline') await resume(state.session)
  }

  const signIn = async (session: StoredSession, user: User) => {
    await saveSession(session)
    setState({ status: 'signed_in', session, user, client: open(session) })
  }

  const signOut = async () => {
    current.current = null
    if (state.status === 'signed_in') await endSession(state.client).catch(() => undefined)
    await end()
  }

  const setUser = (user: User) => setState((current) => (current.status === 'signed_in' ? { ...current, user } : current))

  return <SessionContext value={{ state, signIn, signOut, retry, setUser }}>{children}</SessionContext>
}

export function useSession(): SessionContextValue {
  const value = use(SessionContext)
  if (!value) throw new Error('useSession must be used inside <SessionProvider>')
  return value
}
