import * as SecureStore from 'expo-secure-store'

/** Keychain (iOS) / Keystore (Android). `storage.web.ts` replaces it in the web preview. */
export const storage = {
  get: (key: string) => SecureStore.getItemAsync(key),
  set: (key: string, value: string) => SecureStore.setItemAsync(key, value),
  remove: (key: string) => SecureStore.deleteItemAsync(key),
}
