import { storage } from './storage'

export interface StoredSession {
  server: string
  token: string
}

const KEY = 'orbit.session'

export async function loadSession(): Promise<StoredSession | null> {
  const raw = await storage.get(KEY)
  if (!raw) return null
  try {
    const value = JSON.parse(raw)
    return typeof value?.server === 'string' && typeof value?.token === 'string' ? value : null
  } catch {
    return null
  }
}

export const saveSession = (session: StoredSession) => storage.set(KEY, JSON.stringify(session))

export const clearSession = () => storage.remove(KEY)

const WORKSPACE_KEY = 'orbit.workspace'

/** The workspace the user chose last, so the app opens where they left it. */
export const loadWorkspaceId = () => storage.get(WORKSPACE_KEY)

export const saveWorkspaceId = (id: string) => storage.set(WORKSPACE_KEY, id)
