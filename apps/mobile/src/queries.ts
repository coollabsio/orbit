/** The queries the screens share. Each one reads its workspace and client from `useOrbit()`. */
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useMemo } from 'react'

import { chatBadge, type Person } from './data/chat'
import { keys } from './data/keys'
import * as api from './data/orbit'
import { useOrbit } from './orbit-context'

export function useProjects() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.projects(workspaceId), queryFn: () => api.projects(client, workspaceId) })
}

/** The statuses of every project. Reads the project list through the cache, so a refresh of one refreshes both. */
export function useStatuses() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useQuery({
    queryKey: keys.statuses(workspaceId),
    queryFn: async () => {
      const projects = await queryClient.ensureQueryData({ queryKey: keys.projects(workspaceId), queryFn: () => api.projects(client, workspaceId) })
      return api.statuses(client, workspaceId, projects.map((project) => project.id))
    },
  })
}

/** Every task, or a server view's tasks (`mine`, `my_week`, `overdue` …). */
export function useTasks(view?: string) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: view ? keys.taskView(workspaceId, view) : keys.tasks(workspaceId), queryFn: () => api.tasks(client, workspaceId, view) })
}

export function useLabels() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.labels(workspaceId), queryFn: () => api.labels(client, workspaceId) })
}

export function useMembers() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.members(workspaceId), queryFn: () => api.members(client, workspaceId) })
}

/** Members as `{ id, name }` by user id. Empty while they load. */
export function usePeople(): Person[] {
  const members = useMembers()
  return useMemo(() => (members.data ?? []).map((member) => ({ id: member.user_id, name: member.display_name })), [members.data])
}

export function useConversations() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.conversations(workspaceId), queryFn: () => api.conversations(client, workspaceId) })
}

export function useChatCategories() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.chatCategories(workspaceId), queryFn: () => api.chatCategories(client, workspaceId) })
}

export function useChatStates() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.chatStates(workspaceId), queryFn: () => api.chatStates(client, workspaceId) })
}

/** The Chat badge. Nothing while the data loads. */
export function useChatBadge(): { count: number; unread: boolean } {
  const { user } = useOrbit()
  const conversations = useConversations()
  const states = useChatStates()
  return chatBadge(conversations.data ?? [], states.data ?? [], user.id)
}

export function useNotifications() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.notifications(workspaceId), queryFn: () => api.notifications(client, workspaceId) })
}

export function useUnreadNotifications() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.unreadNotifications(workspaceId), queryFn: () => api.unreadNotifications(client, workspaceId) })
}

export function usePages() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.pages(workspaceId), queryFn: () => api.pages(client, workspaceId) })
}

export function useTeamspaces() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.teamspaces(workspaceId), queryFn: () => api.teamspaces(client, workspaceId) })
}
