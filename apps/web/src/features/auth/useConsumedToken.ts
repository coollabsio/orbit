import { useState } from 'react'
import { consumeQueryToken } from './authFlow'

export function useConsumedToken() {
  const [token] = useState(() => consumeQueryToken(window.location.href, (href) => window.history.replaceState(null, '', href)))
  return token
}
