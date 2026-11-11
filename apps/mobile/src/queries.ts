/** The queries the screens share. Each one reads its workspace and client from `useOrbit()`. */
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useMemo } from 'react'

import { chatBadge, type Person } from './data/chat'
import { keys } from './data/keys'
import type { MentionTarget } from './data/mentions'
import * as api from './data/orbit'
import * as taskApi from './data/task-api'
import { useOrbit } from './orbit-context'

export function useProjects(enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.projects(workspaceId), queryFn: () => api.projects(client, workspaceId), enabled })
}

/** The statuses of every project. Reads the project list through the cache, so a refresh of one refreshes both. */
export function useStatuses(enabled = true) {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  return useQuery({
    queryKey: keys.statuses(workspaceId),
    enabled,
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

/** One project's milestones; off (`null`) on a page across projects. */
export function useMilestones(projectId: string | null) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.milestones(workspaceId, projectId ?? ''), queryFn: () => taskApi.milestones(client, workspaceId, projectId!), enabled: projectId !== null })
}

/** One project's cycles; off (`null`) on a page across projects. */
export function useCycles(projectId: string | null) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.cycles(workspaceId, projectId ?? ''), queryFn: () => taskApi.cycles(client, workspaceId, projectId!), enabled: projectId !== null })
}

export function useMembers() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.members(workspaceId), queryFn: () => api.members(client, workspaceId) })
}

/**
 * Who sees the project `projectId`, for assignee and mention pickers: everyone for a workspace project; for a private
 * one, its listed members plus owners and admins (the server refuses anyone else).
 */
export function useSeesProject(projectId: string | null): (userId: string) => boolean {
  const { client, workspaceId } = useOrbit()
  const project = useProjects().data?.find((candidate) => candidate.id === projectId)
  const isPrivate = project?.private === true
  const members = useMembers().data
  const listed = useQuery({
    queryKey: keys.projectMembers(workspaceId, projectId ?? ''),
    queryFn: () => api.projectMembers(client, workspaceId, projectId ?? ''),
    enabled: isPrivate,
  }).data
  return useMemo(() => {
    if (!isPrivate) return () => true
    const allowed = new Set(listed?.map((member) => member.user_id))
    for (const member of members ?? []) if (member.role === 'owner' || member.role === 'admin') allowed.add(member.user_id)
    return (userId: string) => allowed.has(userId)
  }, [isPrivate, listed, members])
}

/** Members as `{ id, name }` by user id. Empty while they load. */
export function usePeople(): Person[] {
  const members = useMembers()
  return useMemo(() => (members.data ?? []).map((member) => ({ id: member.user_id, name: member.display_name })), [members.data])
}

/** Members who can be mentioned (not suspended), with the email's local part as the handle, as on the web. */
export function useMentionPeople(): MentionTarget[] {
  const members = useMembers()
  return useMemo(
    () => (members.data ?? []).filter((member) => !member.suspended_at).map((member) => ({ id: member.user_id, name: member.display_name, handle: member.email.split('@')[0] ?? member.email })),
    [members.data],
  )
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

/** `enabled` is off for a guest: it has no Activity. */
export function useUnreadNotifications(enabled = true) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.unreadNotifications(workspaceId), queryFn: () => api.unreadNotifications(client, workspaceId), enabled })
}

export function usePages() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.pages(workspaceId), queryFn: () => api.pages(client, workspaceId) })
}

export function useTeamspaces() {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: keys.teamspaces(workspaceId), queryFn: () => api.teamspaces(client, workspaceId) })
}
