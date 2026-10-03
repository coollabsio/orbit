import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { deletePushSubscription, getNotificationPreferences, listPushSubscriptions, putNotificationPreferences, sendTestPush } from '@/api/generated/sdk.gen'
import type { NotificationPrefs, PushSubscriptionRecord } from '@/api/generated/types.gen'
import { currentSubscription, disablePush, enablePush } from '@/features/realtime/push'

/** The browsers the user gets notifications on. */
export function usePushDevices() {
  return useQuery({
    queryKey: queryKeys.pushDevices,
    queryFn: async () => (await listPushSubscriptions({ client: apiClient, throwOnError: true })).data,
  })
}

/** This browser's push endpoint, or `null` while notifications are off here. */
export function useThisDeviceEndpoint() {
  return useQuery({
    queryKey: queryKeys.pushThisDevice,
    queryFn: async () => (await currentSubscription())?.endpoint ?? null,
    staleTime: 0,
  })
}

function useRefreshDevices() {
  const queryClient = useQueryClient()
  return () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.pushDevices })
    void queryClient.invalidateQueries({ queryKey: queryKeys.pushThisDevice })
  }
}

export function useEnablePush() {
  return useMutation({ mutationFn: enablePush, onSettled: useRefreshDevices() })
}

export function useDisablePush() {
  return useMutation({ mutationFn: disablePush, onSettled: useRefreshDevices() })
}

/** Removes a device from the list; this browser also drops its own subscription. */
export function useRemovePushDevice() {
  return useMutation({
    mutationFn: async (device: PushSubscriptionRecord) => {
      const subscription = await currentSubscription()
      if (subscription?.endpoint === device.endpoint) return disablePush()
      await deletePushSubscription({ client: apiClient, path: { id: device.id }, throwOnError: true })
    },
    onSettled: useRefreshDevices(),
  })
}

/** Resolves to the number of devices the test notification went to. */
export function useSendTestPush() {
  return useMutation({
    mutationFn: async () => (await sendTestPush({ client: apiClient, throwOnError: true })).data.sent,
  })
}

export function useNotificationPreferences() {
  return useQuery({
    queryKey: queryKeys.notificationPreferences,
    queryFn: async () => (await getNotificationPreferences({ client: apiClient, throwOnError: true })).data,
  })
}

/** Saves the whole set. The switch moves at once and goes back if the server refuses. */
export function useSaveNotificationPreferences() {
  const queryClient = useQueryClient()
  const key = queryKeys.notificationPreferences
  return useMutation({
    mutationFn: async (body: NotificationPrefs) => (await putNotificationPreferences({ client: apiClient, body, throwOnError: true })).data,
    onMutate: async (body) => {
      await queryClient.cancelQueries({ queryKey: key })
      const previous = queryClient.getQueryData<NotificationPrefs>(key)
      queryClient.setQueryData(key, body)
      return { previous }
    },
    onError: (_error, _body, context) => {
      if (context?.previous) queryClient.setQueryData(key, context.previous)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: key }),
  })
}
