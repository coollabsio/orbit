/** The web preview (`expo start --web`) only: expo-secure-store has no web support. Never ship this as a real web client. */
export const storage = {
  get: async (key: string) => localStorage.getItem(key),
  set: async (key: string, value: string) => localStorage.setItem(key, value),
  remove: async (key: string) => localStorage.removeItem(key),
}
