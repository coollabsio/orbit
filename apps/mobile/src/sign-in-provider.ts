import * as Crypto from 'expo-crypto'
import * as Linking from 'expo-linking'
import * as WebBrowser from 'expo-web-browser'

import { type AuthSessionResult, nativeRedirect, type ProviderDevice } from './data/sign-in-provider'

/** The device parts of a provider sign-in: random bytes and SHA-256 from expo-crypto, the system browser's auth session. */
export const providerDevice: ProviderDevice = {
  randomBytes: (count) => Crypto.getRandomBytes(count),
  sha256: (data) => Crypto.digest(Crypto.CryptoDigestAlgorithm.SHA256, data),
  redirect: () => nativeRedirect(Linking.createURL('auth/callback')),
  openAuthSession: async (url, redirect) => (await WebBrowser.openAuthSessionAsync(url, redirect)) as AuthSessionResult,
}
