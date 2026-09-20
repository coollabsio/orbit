import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import {
  changePassword,
  login,
  logout,
  me,
  recoveryComplete,
  recoveryRequest,
  setupComplete,
  setupStatus,
  updateMe,
} from '@/api/generated/sdk.gen'
import type {
  ChangePasswordBody,
  LoginBody,
  RecoveryCompleteBody,
  RecoveryRequestBody,
  SetupBody,
  UpdateMeBody,
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
  return useQuery({ ...currentUserQueryOptions(), enabled })
}

export function useSetupStatus() {
  return useQuery({
    queryKey: queryKeys.setup,
    queryFn: async () => {
      const { data } = await setupStatus({ client: apiClient, throwOnError: true })
      return required(data, 'Setup status response was empty.')
    },
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
    mutationFn: async () => { await logout({ client: apiClient, throwOnError: true }) },
    onSuccess: () => {
      queryClient.clear()
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

export function useUpdateProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: UpdateMeBody) => {
      const { data } = await updateMe({ client: apiClient, body, throwOnError: true })
      return required(data, 'Profile update response was empty.')
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
