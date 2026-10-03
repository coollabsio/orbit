import { expect, test } from 'bun:test'
import { appPath, base64UrlToBytes, bytesToBase64Url, deviceLabel, needsHomeScreen } from './push'

test('a base64url key becomes its bytes, with or without padding', () => {
  // "-_" are the two characters that differ from plain base64 ("+/").
  expect([...base64UrlToBytes('-_8')]).toEqual([251, 255])
  expect([...base64UrlToBytes('AQID')]).toEqual([1, 2, 3])
  expect([...base64UrlToBytes('AQIDBA')]).toEqual([1, 2, 3, 4])
  expect([...base64UrlToBytes('AQIDBA==')]).toEqual([1, 2, 3, 4])
  // A P-256 public key is 65 bytes: 87 base64url characters, no padding.
  const key = new Uint8Array(65).map((_, index) => (index * 37 + 251) % 256)
  const encoded = bytesToBase64Url(key.buffer)
  expect(encoded.length).toBe(87)
  expect(encoded).not.toMatch(/[+/=]/)
  expect([...base64UrlToBytes(encoded)]).toEqual([...key])
})

const AGENTS = {
  chromeLinux: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  firefoxLinux: 'Mozilla/5.0 (X11; Linux x86_64; rv:143.0) Gecko/20100101 Firefox/143.0',
  safariMac: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15',
  edgeWindows: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0',
  chromeAndroid: 'Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36',
  safariIphone: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Mobile/15E148 Safari/604.1',
}

test('the device label names the browser and the system from the user agent', () => {
  expect(deviceLabel(AGENTS.chromeLinux)).toBe('Chrome on Linux')
  expect(deviceLabel(AGENTS.firefoxLinux)).toBe('Firefox on Linux')
  expect(deviceLabel(AGENTS.safariMac)).toBe('Safari on macOS')
  expect(deviceLabel(AGENTS.edgeWindows)).toBe('Edge on Windows')
  expect(deviceLabel(AGENTS.chromeAndroid)).toBe('Chrome on Android')
  expect(deviceLabel(AGENTS.safariIphone)).toBe('Safari on iOS')
  expect(deviceLabel('')).toBe('Browser')
})

test('the browser hints win over the user agent, which every Chromium browser shares', () => {
  const brave = { brands: [{ brand: 'Not;A=Brand' }, { brand: 'Chromium' }, { brand: 'Brave' }], platform: 'Linux' }
  expect(deviceLabel(AGENTS.chromeLinux, brave)).toBe('Brave on Linux')
  expect(deviceLabel(AGENTS.chromeLinux, { brands: [{ brand: 'Chromium' }, { brand: 'Google Chrome' }], platform: 'Chrome OS' })).toBe('Chrome on ChromeOS')
  // Hints without a known brand or a platform fall back to the user agent.
  expect(deviceLabel(AGENTS.edgeWindows, { brands: [], platform: '' })).toBe('Edge on Windows')
  expect(deviceLabel('x'.repeat(10), { brands: [{ brand: 'Chromium' }], platform: 'y'.repeat(200) }).length).toBe(120)
})

test('only Safari on an iPhone or iPad outside the installed app needs the Home Screen', () => {
  expect(needsHomeScreen(AGENTS.safariIphone, false)).toBe(true)
  expect(needsHomeScreen(AGENTS.safariIphone, true)).toBe(false)
  // An iPad says it is a Mac; it has a touch screen.
  expect(needsHomeScreen(AGENTS.safariMac, false, 5)).toBe(true)
  expect(needsHomeScreen(AGENTS.safariMac, undefined, 0)).toBe(false)
  expect(needsHomeScreen(AGENTS.chromeLinux, undefined)).toBe(false)
})

test('a notification opens app paths only', () => {
  expect(appPath('/chat/c1?thread=m1')).toBe('/chat/c1?thread=m1')
  expect(appPath('https://example.com/')).toBe('/')
  expect(appPath('//example.com/')).toBe('/')
  // the URL parser reads these as another site
  expect(appPath('/\\example.com/')).toBe('/')
  expect(appPath('/\t/example.com/')).toBe('/')
  expect(appPath(undefined)).toBe('/')
})
