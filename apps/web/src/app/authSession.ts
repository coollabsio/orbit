import type { QueryClient } from '@tanstack/react-query'
import { clearLocalCache } from '@/lib/localCache'
import { loginPath } from '@/features/auth/returnTo'

export async function clearExpiredSession(
  queryClient: QueryClient,
  navigate: (path: string) => void,
  // The page the person was on, to open again after the next sign-in.
  returnTo = '/',
) {
  await queryClient.cancelQueries({ queryKey: ['current-user'] })
  queryClient.removeQueries({ queryKey: ['current-user'] })
  // Whoever signs in next may be another user.
  void clearLocalCache()
  navigate(loginPath(returnTo))
}
