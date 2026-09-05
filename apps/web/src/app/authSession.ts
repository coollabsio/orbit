import type { QueryClient } from '@tanstack/react-query'

export async function clearExpiredSession(
  queryClient: QueryClient,
  navigate: (path: string) => void,
) {
  await queryClient.cancelQueries({ queryKey: ['current-user'] })
  queryClient.removeQueries({ queryKey: ['current-user'] })
  navigate('/login')
}
