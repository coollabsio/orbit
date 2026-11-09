import '../global.css'
import { NotoSans_400Regular } from '@expo-google-fonts/noto-sans/400Regular'
import { NotoSans_500Medium } from '@expo-google-fonts/noto-sans/500Medium'
import { NotoSans_600SemiBold } from '@expo-google-fonts/noto-sans/600SemiBold'
import { NotoSans_700Bold } from '@expo-google-fonts/noto-sans/700Bold'
import { focusManager, QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { DarkTheme, DefaultTheme, type ErrorBoundaryProps, SplashScreen, Stack, ThemeProvider } from 'expo-router'
import { StatusBar } from 'expo-status-bar'
import * as SystemUI from 'expo-system-ui'
import { useFonts } from 'expo-font'
import { useEffect, useState } from 'react'
import { AppState, View } from 'react-native'

import { NoWorkspace } from '../components/NoWorkspace'
import { OrbitProvider } from '../orbit-context'
import { SessionProvider, useSession } from '../session-context'
import { fonts, loadAppearance, palettes, type Theme, useTheme } from '../theme'
import { AppColorScope, useAppPalette } from '../theme/ColorScope'
import { Button, EmptyState } from '../ui'

void SplashScreen.preventAutoHideAsync()

// Live sockets keep the data fresh; a refetch on every mount would only repeat it.
const queryClient = new QueryClient({ defaultOptions: { queries: { staleTime: 30_000 } } })

// React Native has no window focus: coming back to the foreground refetches stale queries.
focusManager.setEventListener((setFocused) => {
  const subscription = AppState.addEventListener('change', (state) => setFocused(state === 'active'))
  return () => subscription.remove()
})

function navigationTheme({ scheme, colors }: Theme) {
  const base = scheme === 'dark' ? DarkTheme : DefaultTheme
  return {
    ...base,
    colors: { ...base.colors, primary: colors.primary, background: colors.background, card: colors.background, text: colors.foreground, border: colors.border, notification: colors.primary },
    fonts: {
      regular: { fontFamily: fonts.regular, fontWeight: 'normal' as const },
      medium: { fontFamily: fonts.medium, fontWeight: 'normal' as const },
      bold: { fontFamily: fonts.semibold, fontWeight: 'normal' as const },
      heavy: { fontFamily: fonts.bold, fontWeight: 'normal' as const },
    },
  }
}

/** A render error anywhere in the app: expo-router shows this in place of the root layout, and Try again renders it again. */
export function ErrorBoundary({ retry }: ErrorBoundaryProps) {
  // the providers are gone with the layout, so the splash screen may still be up
  useEffect(() => {
    SplashScreen.hide()
  }, [])
  return (
    <View className="flex-1 bg-background">
      <EmptyState
        icon="danger"
        title="Something went wrong"
        description="This part of Orbit stopped working. Try again, or restart the app."
        action={<Button label="Try again" onPress={() => void retry()} />}
      />
    </View>
  )
}

export default function RootLayout() {
  const [fontsLoaded, fontError] = useFonts({ NotoSans_400Regular, NotoSans_500Medium, NotoSans_600SemiBold, NotoSans_700Bold })
  return (
    <QueryClientProvider client={queryClient}>
      <SessionProvider>
        <RootNavigator fontsReady={fontsLoaded || fontError !== null} />
      </SessionProvider>
    </QueryClientProvider>
  )
}

function RootNavigator({ fontsReady }: { fontsReady: boolean }) {
  const { state } = useSession()
  // the root is above `AppColorScope`, so it reads the app's colours itself (with the custom theme)
  const palette = useAppPalette()
  const theme = { ...useTheme(), scheme: palette.scheme, colors: { ...palettes[palette.scheme], ...palette.colors } }
  const [schemeLoaded, setSchemeLoaded] = useState(false)
  const ready = fontsReady && schemeLoaded && state.status !== 'loading'

  useEffect(() => {
    void loadAppearance().finally(() => setSchemeLoaded(true))
  }, [])
  const signedIn = state.status === 'signed_in'

  useEffect(() => {
    // the root view shows behind screen transitions and the keyboard; keep it on the theme canvas
    void SystemUI.setBackgroundColorAsync(theme.colors.background)
  }, [theme.colors.background])

  useEffect(() => {
    if (ready) SplashScreen.hide()
  }, [ready])

  if (!ready) return null

  const stack = (
    <Stack
      screenOptions={{
        headerShown: false,
        contentStyle: { backgroundColor: theme.colors.background },
        headerStyle: { backgroundColor: theme.colors.background },
        headerShadowVisible: false,
        headerTintColor: theme.colors.foreground,
        headerTitleStyle: { fontFamily: theme.fonts.semibold, fontSize: 17, color: theme.colors.foreground },
        headerBackButtonDisplayMode: 'minimal',
      }}
    >
      <Stack.Protected guard={signedIn}>
        {/* signing in or out swaps the whole app: no push animation */}
        <Stack.Screen name="(tabs)" options={{ animation: 'none' }} />
        <Stack.Screen name="chat/[id]" />
        <Stack.Screen name="chat/[id]/members" />
        <Stack.Screen name="chat/[id]/search" />
        <Stack.Screen name="chat/[id]/pins" />
        <Stack.Screen name="chat/[id]/files" />
        <Stack.Screen name="chat/[id]/threads" />
        <Stack.Screen name="chat/thread/[id]" />
        <Stack.Screen name="chat/unreads" />
        <Stack.Screen name="chat/threads" />
        <Stack.Screen name="chat/browse" />
      </Stack.Protected>
      <Stack.Protected guard={!signedIn}>
        <Stack.Screen name="sign-in" options={{ animation: 'none' }} />
      </Stack.Protected>
    </Stack>
  )

  return (
    <AppColorScope>
      <ThemeProvider value={navigationTheme(theme)}>
        <StatusBar style={theme.scheme === 'dark' ? 'light' : 'dark'} />
        {state.status === 'offline' ? <SessionOffline server={state.session.server} /> : signedIn ? <OrbitProvider noWorkspace={<NoWorkspace />}>{stack}</OrbitProvider> : stack}
      </ThemeProvider>
    </AppColorScope>
  )
}

/** The saved session could not be checked at launch (offline, a server error): the user stays signed in. */
function SessionOffline({ server }: { server: string }) {
  const { retry, signOut } = useSession()
  const [retrying, setRetrying] = useState(false)
  return (
    <View className="flex-1 bg-background">
      <EmptyState
        icon="danger"
        title="Orbit is unavailable"
        description={`${server.replace(/^https?:\/\//, '')} could not verify this session. Check your connection and retry.`}
        action={
          <View className="items-center gap-2">
            <Button label="Retry" loading={retrying} onPress={() => {
              setRetrying(true)
              void retry().finally(() => setRetrying(false))
            }} />
            <Button label="Sign out" variant="ghost" muted onPress={() => void signOut()} />
          </View>
        }
      />
    </View>
  )
}
