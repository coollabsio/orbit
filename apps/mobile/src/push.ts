/**
 * Native push notifications straight through APNs on iOS and FCM on Android: the server sends to Apple or Google with
 * this device's token, no Expo push service. One query (`thisDeviceKey`) knows whether this device gets notifications;
 * reading it registers the token again when the system already allows notifications, since a token can change. Its key has the workspace, so
 * `usePushNotifications` (in the tabs layout) runs it on launch, sign-in and workspace change; it also opens the screen
 * of a tapped notification.
 */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import * as Application from 'expo-application'
import Constants, { ExecutionEnvironment } from 'expo-constants'
import * as Device from 'expo-device'
import * as Notifications from 'expo-notifications'
import { router } from 'expo-router'
import { useEffect } from 'react'
import { Platform } from 'react-native'

import * as account from './data/account-api'
import type { OrbitClient } from './data/orbit'
import { pushRoute, pushUrl } from './data/push-route'
import { useOrbit } from './orbit-context'
import { storage } from './storage'

/** What the "This device" card shows. */
export type ThisDevice =
  /** Web, or Expo Go: Expo Go's token belongs to Expo Go, not to Orbit. */
  | { state: 'unsupported'; reason: 'expo-go' | 'platform' }
  /** The system settings turned notifications off for Orbit; the app cannot ask again. */
  | { state: 'denied' }
  /** Not asked yet, or turned off here (the device was removed). */
  | { state: 'off' }
  /** The server has no APNs key (iOS) or no FCM service account (Android). */
  | { state: 'not_configured' }
  | { state: 'on'; deviceId: string }

/** Which push service a phone gets notifications from: APNs for iOS, FCM for Android. */
export type PhoneKind = 'apns' | 'fcm'

export const thisDeviceKey = (workspaceId: string) => ['account', 'push-this-device', workspaceId] as const

/** The Android notification channel the server names in each push (`CHANNEL_ID` in the server's `push/fcm.rs`). */
const ANDROID_CHANNEL = 'default'

/** Set when the user removed this device: launches do not add it back until Turn on. */
const OPTED_OUT = 'orbit.push.opted-out'

const unsupported = (): ThisDevice | null => {
  if (Platform.OS !== 'ios' && Platform.OS !== 'android') return { state: 'unsupported', reason: 'platform' }
  if (Constants.executionEnvironment === ExecutionEnvironment.StoreClient) return { state: 'unsupported', reason: 'expo-go' }
  return null
}

// While the app is open its live socket counts as a tab the user is at, so the server sends no push then; what still
// arrives (the test notification, one racing the app to the foreground) shows as a banner.
if (!unsupported()) {
  Notifications.setNotificationHandler({
    handleNotification: async () => ({ shouldShowBanner: true, shouldShowList: true, shouldPlaySound: true, shouldSetBadge: false }),
  })
}

/** `iPhone 16 Pro · iOS 26.1` or `Pixel 9 · Android 16`, as the Devices list names it. */
const deviceLabel = () =>
  Platform.OS === 'android'
    ? `${Device.modelName ?? 'Android phone'} · Android ${Device.osVersion ?? Platform.Version}`
    : `${Device.modelName ?? (Device.deviceType === Device.DeviceType.TABLET ? 'iPad' : 'iPhone')} · iOS ${Device.osVersion ?? Platform.Version}`

/** Android 13 and later ask for the permission only when a channel exists; making it again changes nothing. */
async function prepareChannel() {
  if (Platform.OS !== 'android') return
  await Notifications.setNotificationChannelAsync(ANDROID_CHANNEL, { name: 'Notifications', importance: Notifications.AndroidImportance.HIGH })
}

/** The APNs environment of this build's provisioning profile: development for a dev build, production for TestFlight. */
async function environment(): Promise<account.ApnsEnvironment> {
  const fromProfile = await Application.getIosPushNotificationServiceEnvironmentAsync().catch(() => null)
  if (fromProfile) return fromProfile === 'production' ? 'production' : 'sandbox'
  return __DEV__ ? 'sandbox' : 'production'
}

async function register(client: OrbitClient): Promise<ThisDevice> {
  try {
    const { data: token } = await Notifications.getDevicePushTokenAsync()
    const device =
      Platform.OS === 'android'
        ? await account.registerFcmDevice(client, { token: String(token), label: deviceLabel() })
        : await account.registerApnsDevice(client, { token: String(token), environment: await environment(), label: deviceLabel() })
    return { state: 'on', deviceId: device.id }
  } catch (error) {
    if (account.pushNotConfigured(error)) return { state: 'not_configured' }
    throw error
  }
}

/** Turned off in the system settings. Android can ask once more after the first "Don't allow"; iOS cannot. */
const blocked = (permission: Notifications.NotificationPermissionsStatus) => permission.status === 'denied' && !permission.canAskAgain

/** Reads the permission and, when it is granted, registers the token again. Never asks. */
async function syncThisDevice(client: OrbitClient): Promise<ThisDevice> {
  const skip = unsupported()
  if (skip) return skip
  await prepareChannel()
  const permission = await Notifications.getPermissionsAsync()
  if (blocked(permission)) return { state: 'denied' }
  if (!permission.granted || (await storage.get(OPTED_OUT)) === '1') return { state: 'off' }
  return register(client)
}

/** This device's state; opening the Notifications page reads it again (Settings may have changed meanwhile). */
export function useThisDevice({ refresh = false } = {}) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: thisDeviceKey(workspaceId), queryFn: () => syncThisDevice(client), staleTime: Infinity, refetchOnMount: refresh ? 'always' : false })
}

/** Turn on: asks the system (the first time), then registers. */
export function useTurnOnPush() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (): Promise<ThisDevice> => {
      await prepareChannel()
      const permission = await Notifications.requestPermissionsAsync()
      if (!permission.granted) return blocked(permission) ? { state: 'denied' } : { state: 'off' }
      await storage.remove(OPTED_OUT)
      return register(client)
    },
    onSuccess: (device) => queryClient.setQueryData(thisDeviceKey(workspaceId), device),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: phoneDevicesKey(Platform.OS === 'android' ? 'fcm' : 'apns') }),
  })
}

const phoneDevicesKey = (kind: PhoneKind) => (kind === 'fcm' ? account.accountKeys.fcmDevices : account.accountKeys.apnsDevices)

/** Removes a phone from the list. Removing this one also keeps launches from adding it back. */
export function useRemovePhoneDevice() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ kind, id }: { kind: PhoneKind; id: string }) => {
      await (kind === 'fcm' ? account.removeFcmDevice(client, id) : account.removeApnsDevice(client, id))
      const here = queryClient.getQueryData<ThisDevice>(thisDeviceKey(workspaceId))
      if (here?.state === 'on' && here.deviceId === id) {
        await storage.set(OPTED_OUT, '1')
        queryClient.setQueryData<ThisDevice>(thisDeviceKey(workspaceId), { state: 'off' })
      }
    },
    onSettled: (_data, _error, { kind }) => void queryClient.invalidateQueries({ queryKey: phoneDevicesKey(kind) }),
  })
}

/** Opens a tapped notification's screen once, and forgets it so a later sign-in does not open it again. */
function open(response: Notifications.NotificationResponse) {
  Notifications.clearLastNotificationResponse()
  if (response.actionIdentifier !== Notifications.DEFAULT_ACTION_IDENTIFIER) return
  const url = pushUrl(response.notification.request)
  const route = url ? pushRoute(url) : null
  if (route) router.push(route as Parameters<typeof router.push>[0])
}

/**
 * Mounted once in the signed-in app: registers this device again on launch, sign-in and workspace change, follows a
 * new token, and opens tapped notifications (also the one that launched the app).
 */
export function usePushNotifications() {
  const { workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const on = useThisDevice().data?.state === 'on'

  useEffect(() => {
    if (!on) return
    const subscription = Notifications.addPushTokenListener(() => void queryClient.invalidateQueries({ queryKey: thisDeviceKey(workspaceId) }))
    return () => subscription.remove()
  }, [on, queryClient, workspaceId])

  useEffect(() => {
    if (unsupported()) return
    const last = Notifications.getLastNotificationResponse()
    if (last) open(last)
    const subscription = Notifications.addNotificationResponseReceivedListener(open)
    return () => subscription.remove()
  }, [])
}
