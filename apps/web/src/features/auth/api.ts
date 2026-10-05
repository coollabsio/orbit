import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { apiClient, type createApiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { disablePushForSignOut } from '@/features/realtime/push'
import { clearViewSessionEdits } from '@/features/views/useViewState'
import { clearLocalCache, withinTime } from '@/lib/localCache'
import { connectionRetryDelay, retryConnectionFailure } from '@/lib/connection'
import {
  changePassword,
  login,
  logout,
  me,
  putStatus,
  recoveryComplete,
  recoveryRequest,
  removeAvatar,
  setupComplete,
  setupStatus,
  updateMe,
  uploadAvatar,
} from '@/api/generated/sdk.gen'
import type {
  ChangePasswordBody,
  LoginBody,
  RecoveryCompleteBody,
  RecoveryRequestBody,
  SetupBody,
  AuthUserResponse,
  UpdateMeBody,
  UserStatus,
} from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'

type ApiClient = ReturnType<typeof createApiClient>

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export function currentUserQueryOptions(client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.currentUser,
    queryFn: async () => {
      try {
        const { data } = await me({ client, throwOnError: true })
        return required(data, 'Current user response was empty.')
      } catch (error) {
        if (error instanceof ApiProblem && error.status === 401) return null
        throw error
      }
    },
  }
}

export function useCurrentUser(enabled = true) {
  // A core query: a network or 5xx failure retries with backoff, so a blip does not end the session on screen.
  return useQuery({ ...currentUserQueryOptions(), enabled, retry: retryConnectionFailure, retryDelay: connectionRetryDelay })
}

export function useSetupStatus() {
  return useQuery({
    queryKey: queryKeys.setup,
    queryFn: async () => {
      const { data } = await setupStatus({ client: apiClient, throwOnError: true })
      return required(data, 'Setup status response was empty.')
    },
    retry: retryConnectionFailure,
    retryDelay: connectionRetryDelay,
  })
}

export function useLogin() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: LoginBody) => {
      const { data } = await login({ client: apiClient, body, throwOnError: true })
      return required(data, 'Login response was empty.')
    },
    onSuccess: (response) => {
      queryClient.setQueryData(queryKeys.currentUser, response.user)
      void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
    },
  })
}

export function useLogout() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async () => {
      // While the session still lets the server forget this browser.
      await disablePushForSignOut()
      await logout({ client: apiClient, throwOnError: true })
    },
    onSuccess: async () => {
      queryClient.clear()
      clearViewSessionEdits()
      // What this user's chat left on the device does not stay for the next person.
      await withinTime(clearLocalCache())
    },
  })
}

export function useCompleteSetup() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: SetupBody) => {
      const { data } = await setupComplete({ client: apiClient, body, throwOnError: true })
      return required(data, 'Setup response was empty.')
    },
    onSuccess: () => {
      queryClient.setQueryData(queryKeys.setup, { complete: true })
      void queryClient.invalidateQueries({ queryKey: queryKeys.currentUser })
      void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
    },
  })
}

export function useRequestRecovery() {
  return useMutation({
    mutationFn: async (body: RecoveryRequestBody) => {
      const { data } = await recoveryRequest({ client: apiClient, body, throwOnError: true })
      return required(data, 'Recovery response was empty.')
    },
  })
}

export function useCompleteRecovery() {
  return useMutation({
    mutationFn: async (body: RecoveryCompleteBody) => {
      await recoveryComplete({ client: apiClient, body, throwOnError: true })
    },
  })
}

/**
 * Saves the profile. The body has the name and only the optional parts (title, pronouns, time zone, bio) that changed:
 * an absent part stays as it is, an empty text removes it (`profileChanges` builds it).
 */
export function useUpdateProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: UpdateMeBody) => {
      const { data } = await updateMe({ client: apiClient, body, throwOnError: true })
      return required(data, 'Profile update response was empty.')
    },
    onSuccess: (user) => {
      queryClient.setQueryData(queryKeys.currentUser, user)
      // the name is in the members lists, the other parts in the profile that the popover reads (one for each workspace)
      void queryClient.invalidateQueries({
        predicate: ({ queryKey }) => queryKey.at(-1) === 'members' || (queryKey.at(-2) === 'profiles' && queryKey.at(-1) === user.id),
      })
    },
  })
}

/** Upload (a 256 px square from `avatarImage`) or remove (`null`) the profile picture. */
export function useSetAvatar() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (image: Blob | null) => {
      const { data } = image
        ? await uploadAvatar({ client: apiClient, body: { file: image }, throwOnError: true })
        : await removeAvatar({ client: apiClient, throwOnError: true })
      return required(data, 'Avatar response was empty.')
    },
    onSuccess: (user: AuthUserResponse) => {
      queryClient.setQueryData(queryKeys.currentUser, user)
      // members lists carry the avatar URL; realtime refreshes them for everybody else
      void queryClient.invalidateQueries({ predicate: (query) => query.queryKey.at(-1) === 'members' })
    },
  })
}

/**
 * Sets the presence and the custom status together. The menu shows the choice at once; a refused request puts the
 * previous status back and says so.
 */
export function useSetStatus() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: UserStatus) => {
      const { data } = await putStatus({ client: apiClient, body, throwOnError: true })
      return required(data, 'Status response was empty.')
    },
    onMutate: async (status) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.currentUser })
      const previous = queryClient.getQueryData<AuthUserResponse | null>(queryKeys.currentUser)
      if (previous) queryClient.setQueryData<AuthUserResponse>(queryKeys.currentUser, { ...previous, status })
      return { previous }
    },
    onError: (_error, _status, context) => {
      if (context?.previous) queryClient.setQueryData(queryKeys.currentUser, context.previous)
      toast.error('Could not update your status. Try again.')
    },
    onSuccess: (user) => {
      queryClient.setQueryData(queryKeys.currentUser, user)
    },
  })
}

export function useChangePassword() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: ChangePasswordBody) => {
      await changePassword({ client: apiClient, body, throwOnError: true })
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.sessions })
    },
  })
}
