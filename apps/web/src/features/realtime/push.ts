import { apiClient } from '@/api/client'
import { createPushSubscription, deletePushSubscription, getPushKey, listPushSubscriptions } from '@/api/generated/sdk.gen'
import { appNavigate } from '@/lib/navigateBridge'

/** The server's base64url key as the bytes `pushManager.subscribe` takes. */
export function base64UrlToBytes(value: string): Uint8Array<ArrayBuffer> {
  const base64 = value.replace(/-/g, '+').replace(/_/g, '/')
  const binary = atob(base64 + '='.repeat((4 - (base64.length % 4)) % 4))
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes
}

export function bytesToBase64Url(buffer: ArrayBuffer | null): string {
  if (!buffer) return ''
  let binary = ''
  for (const byte of new Uint8Array(buffer)) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

/** What `navigator.userAgentData` gives (Chromium only). */
export interface UserAgentHints {
  brands?: { brand: string }[]
  platform?: string
}

const BRAND_NAMES: [brand: string, name: string][] = [
  ['Microsoft Edge', 'Edge'],
  ['Opera', 'Opera'],
  ['Brave', 'Brave'],
  ['Vivaldi', 'Vivaldi'],
  ['Google Chrome', 'Chrome'],
  ['Chromium', 'Chromium'],
]

// The order matters: Edge and Opera also say "Chrome", every Chromium browser also says "Safari".
const AGENT_BROWSERS: [pattern: RegExp, name: string][] = [
  [/Edg(e|A|iOS)?\//, 'Edge'],
  [/OPR\/|Opera/, 'Opera'],
  [/Firefox\/|FxiOS\//, 'Firefox'],
  [/Chrome\/|CriOS\//, 'Chrome'],
  [/Safari\//, 'Safari'],
]

// Android says "Linux" too, and an iPhone says "like Mac OS X".
const AGENT_SYSTEMS: [pattern: RegExp, name: string][] = [
  [/iPhone|iPod/, 'iOS'],
  [/iPad/, 'iPadOS'],
  [/Android/, 'Android'],
  [/Windows/, 'Windows'],
  [/CrOS/, 'ChromeOS'],
  [/Mac OS X|Macintosh/, 'macOS'],
  [/Linux|X11/, 'Linux'],
]

/** The name of this browser in the list of devices, such as "Firefox on Linux". At most 120 characters. */
export function deviceLabel(userAgent: string, hints?: UserAgentHints | null): string {
  const brands = hints?.brands?.map((entry) => entry.brand) ?? []
  const browser = BRAND_NAMES.find(([brand]) => brands.includes(brand))?.[1] ?? AGENT_BROWSERS.find(([pattern]) => pattern.test(userAgent))?.[1]
  const system = (hints?.platform === 'Chrome OS' ? 'ChromeOS' : hints?.platform) || AGENT_SYSTEMS.find(([pattern]) => pattern.test(userAgent))?.[1]
  const label = browser && system ? `${browser} on ${system}` : (browser ?? system ?? 'Browser')
  return label.slice(0, 120)
}

/**
 * Safari on an iPhone or iPad gets notifications only in the app that "Add to Home Screen" makes. `standalone` is
 * `false` in a Safari tab, `true` in the installed app and missing everywhere else. An iPad says "Macintosh".
 */
export function needsHomeScreen(userAgent: string, standalone: boolean | undefined, maxTouchPoints = 0): boolean {
  const ios = /iPhone|iPad|iPod/.test(userAgent) || (/Macintosh/.test(userAgent) && maxTouchPoints > 1)
  return ios && standalone === false
}

export function pushSupported(): boolean {
  return typeof navigator !== 'undefined' && 'serviceWorker' in navigator && typeof window !== 'undefined' && 'PushManager' in window && 'Notification' in window
}

/** An app path: what a notification opens never leaves Orbit. */
export function appPath(url: unknown): string {
  if (typeof url !== 'string' || !url.startsWith('/')) return '/'
  try {
    // The parser decides: `/\\host` and a path with a tab in it name another site.
    const origin = globalThis.location?.origin ?? 'http://localhost'
    const parsed = new URL(url, origin)
    return parsed.origin === origin ? parsed.pathname + parsed.search + parsed.hash : '/'
  } catch {
    return '/'
  }
}

let registration: Promise<ServiceWorkerRegistration> | null = null

function registerWorker(): Promise<ServiceWorkerRegistration> {
  registration ??= navigator.serviceWorker.register('/sw.js').catch((error) => {
    registration = null
    throw error
  })
  return registration
}

/**
 * Registers the worker that shows notifications (this never asks for permission) and follows its "open this page"
 * messages, which it sends when a notification is clicked. Returns the function that stops listening.
 */
export function startPushWorker(): () => void {
  if (typeof navigator === 'undefined' || !('serviceWorker' in navigator)) return () => {}
  void registerWorker().catch(() => {})
  const onMessage = (event: MessageEvent) => {
    const data = event.data as { type?: unknown; url?: unknown } | null
    if (data?.type === 'orbit:navigate') appNavigate(appPath(data.url))
  }
  navigator.serviceWorker.addEventListener('message', onMessage)
  return () => navigator.serviceWorker.removeEventListener('message', onMessage)
}

/** The worker's registration if there is one; never waits for one to appear. */
export async function workerRegistration(): Promise<ServiceWorkerRegistration | null> {
  if (typeof navigator === 'undefined' || !('serviceWorker' in navigator)) return null
  try {
    return (await navigator.serviceWorker.getRegistration('/')) ?? null
  } catch {
    return null
  }
}

/** This browser's push subscription, or `null` when notifications are off here. */
export async function currentSubscription(): Promise<PushSubscription | null> {
  if (!pushSupported()) return null
  const worker = await workerRegistration()
  return (await worker?.pushManager.getSubscription()) ?? null
}

export type EnablePushResult = 'enabled' | 'blocked' | 'dismissed'

/**
 * Turns notifications on for this browser. Call it from a click: it asks for the permission. `blocked` when the user
 * (or the browser) said no, `dismissed` when the question was closed without an answer.
 */
export async function enablePush(): Promise<EnablePushResult> {
  const permission = await Notification.requestPermission()
  if (permission !== 'granted') return permission === 'denied' ? 'blocked' : 'dismissed'
  await registerWorker()
  const worker = await navigator.serviceWorker.ready
  const { data: key } = await getPushKey({ client: apiClient, throwOnError: true })
  const options = { userVisibleOnly: true, applicationServerKey: base64UrlToBytes(key.public_key) }
  let subscription: PushSubscription
  try {
    subscription = await worker.pushManager.subscribe(options)
  } catch (error) {
    // The browser still holds a subscription for a key the server no longer uses.
    if (!(error instanceof DOMException) || error.name !== 'InvalidStateError') throw error
    await (await worker.pushManager.getSubscription())?.unsubscribe()
    subscription = await worker.pushManager.subscribe(options)
  }
  try {
    await createPushSubscription({
      client: apiClient,
      throwOnError: true,
      body: {
        endpoint: subscription.endpoint,
        p256dh: bytesToBase64Url(subscription.getKey('p256dh')),
        auth: bytesToBase64Url(subscription.getKey('auth')),
        label: deviceLabel(navigator.userAgent, (navigator as { userAgentData?: UserAgentHints }).userAgentData),
      },
    })
  } catch (error) {
    // The server does not know this browser, so a subscription here would only look like "on".
    await subscription.unsubscribe().catch(() => {})
    throw error
  }
  return 'enabled'
}

/** Turns notifications off for this browser: the server forgets it, then the browser drops its subscription. */
export async function disablePush(): Promise<void> {
  const subscription = await currentSubscription()
  if (!subscription) return
  const { data: devices } = await listPushSubscriptions({ client: apiClient, throwOnError: true })
  const device = devices.find((entry) => entry.endpoint === subscription.endpoint)
  if (device) await deletePushSubscription({ client: apiClient, path: { id: device.id }, throwOnError: true })
  await subscription.unsubscribe()
}

/**
 * Before sign-out: the next person at this browser must not get this user's notifications. Best effort and bounded,
 * so a slow or failing step never keeps the user signed in.
 */
export async function disablePushForSignOut(waitMs = 3000): Promise<void> {
  if (!pushSupported()) return
  let timer: ReturnType<typeof setTimeout> | undefined
  const timeout = new Promise<void>((resolve) => {
    timer = setTimeout(resolve, waitMs)
  })
  const forget = disablePush()
    // The server could not be told; this browser at least stops listening.
    .catch(async () => void (await (await currentSubscription())?.unsubscribe()))
    .catch(() => {})
  await Promise.race([forget, timeout])
  clearTimeout(timer)
}
