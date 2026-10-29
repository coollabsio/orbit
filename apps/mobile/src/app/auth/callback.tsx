import { router } from 'expo-router'
import { useEffect } from 'react'

/**
 * On Android the provider's redirect (`orbit://auth/callback?handoff=…`) also reaches the router, while the auth session
 * reads the same link. This route only steps back to the screen that started the sign-in.
 */
export default function AuthCallback() {
  useEffect(() => {
    if (router.canGoBack()) router.back()
    else router.replace('/')
  }, [])
  return null
}
