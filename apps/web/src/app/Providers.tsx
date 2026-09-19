import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState, type ReactNode } from 'react'
import { BrowserRouter, useNavigate } from 'react-router'
import { UNAUTHORIZED_EVENT } from '../api/client'
import { ThemeProvider } from '../lib/theme'
import { TooltipProvider } from '../components/ui/tooltip'
import { clearExpiredSession } from './authSession'

function UnauthorizedSessionHandler() {
  const queryClient = useQueryClient()
  const navigate = useNavigate()

  useEffect(() => {
    const unauthorized = () => void clearExpiredSession(queryClient, (path) => navigate(path, { replace: true }))
    window.addEventListener(UNAUTHORIZED_EVENT, unauthorized)
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, unauthorized)
  }, [navigate, queryClient])

  return null
}

export function Providers({ children }: { children: ReactNode }) {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: { retry: false, staleTime: 30_000 },
          mutations: { retry: false },
        },
      }),
  )

  return (
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <UnauthorizedSessionHandler />
          <TooltipProvider>{children}</TooltipProvider>
        </BrowserRouter>
      </QueryClientProvider>
    </ThemeProvider>
  )
}
