import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import {
  deletePasskey,
  disableTwoFactor,
  enableTotp,
  finishPasskeyRegistration,
  getTwoFactor,
  listPasskeys,
  regenerateRecoveryCodes,
  setupTotp,
  startPasskeyRegistration,
} from '@/api/generated/sdk.gen'
import { createPasskey } from '@/features/auth/passkey'

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export function useTwoFactor() {
  return useQuery({
    queryKey: queryKeys.twoFactor,
    queryFn: async () => {
      const { data } = await getTwoFactor({ client: apiClient, throwOnError: true })
      return required(data, 'Two-factor response was empty.')
    },
  })
}

/** Starts the authenticator app setup: a new secret and its QR code. Asks for the current password. */
export function useSetupTotp() {
  return useMutation({
    mutationFn: async (password: string) => {
      const { data } = await setupTotp({ client: apiClient, body: { password }, throwOnError: true })
      return required(data, 'Setup response was empty.')
    },
  })
}

/** Turns the authenticator app on with a code it shows; answers with the recovery codes. */
export function useEnableTotp() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (code: string) => {
      const { data } = await enableTotp({ client: apiClient, body: { code }, throwOnError: true })
      return required(data, 'Recovery codes response was empty.').recovery_codes
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.twoFactor }),
  })
}

export function useDisableTwoFactor() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (password: string) => {
      await disableTwoFactor({ client: apiClient, body: { password }, throwOnError: true })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.twoFactor }),
  })
}

/** New recovery codes; the old ones stop working. */
export function useRegenerateRecoveryCodes() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (password: string) => {
      const { data } = await regenerateRecoveryCodes({ client: apiClient, body: { password }, throwOnError: true })
      return required(data, 'Recovery codes response was empty.').recovery_codes
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.twoFactor }),
  })
}

export function usePasskeys() {
  return useQuery({
    queryKey: queryKeys.passkeys,
    queryFn: async () => {
      const { data } = await listPasskeys({ client: apiClient, throwOnError: true })
      return required(data, 'Passkeys response was empty.')
    },
  })
}

/**
 * The whole ceremony: the server's challenge, the browser prompt, then the server saves the passkey. `beforePrompt`
 * runs once the password is accepted, to close the dialog: a modal makes the page inert, and password manager
 * extensions such as Enpass show their prompt in the page, so the browser's own (iCloud Keychain) would take over.
 */
export function useAddPasskey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async ({ name, password, beforePrompt }: { name: string; password: string; beforePrompt: () => Promise<void> }) => {
      const { data: challenge } = await startPasskeyRegistration({ client: apiClient, body: { password }, throwOnError: true })
      await beforePrompt()
      const credential = await createPasskey(required(challenge, 'Passkey challenge was empty.').options)
      const { data } = await finishPasskeyRegistration({
        client: apiClient,
        body: { challenge_token: challenge.challenge_token, name, credential },
        throwOnError: true,
      })
      return required(data, 'Passkey response was empty.')
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.passkeys }),
  })
}

export function useDeletePasskey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (id: string) => {
      await deletePasskey({ client: apiClient, path: { id }, throwOnError: true })
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: queryKeys.passkeys }),
  })
}
