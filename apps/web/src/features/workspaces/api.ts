import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiClient, type createApiClient } from '@/api/client'
import { fetchAllPages } from '@/api/pagination'
import { queryKeys } from '@/api/queryKeys'
import {
  acceptInvitation,
  previewInvitation,
  changeMemberRole,
  createInvitation,
  createWorkspace,
  deleteWorkspace,
  listInvitations,
  listMembers,
  listWorkspaces,
  removeMember,
  renameWorkspace,
  revokeInvitation,
  transferOwnership,
} from '@/api/generated/sdk.gen'
import type { AcceptBody, InvitationBody, MemberRecord, RoleBody, WorkspaceRecord } from '@/api/generated/types.gen'
import type { User } from './models'

type ApiClient = ReturnType<typeof createApiClient>

function required<T>(data: T | undefined, message: string): T {
  if (data === undefined) throw new Error(message)
  return data
}

export function workspacesQueryOptions(client: ApiClient = apiClient) {
  return {
    queryKey: queryKeys.workspaces,
    queryFn: async () => {
      const { data } = await listWorkspaces({ client, throwOnError: true })
      return required(data, 'Workspaces response was empty.')
    },
  }
}

export function memberFromRecord(member: MemberRecord): User {
  const hue = [...member.user_id].reduce((total, character) => total + character.charCodeAt(0), 0) % 360
  const role = member.role.charAt(0).toUpperCase() + member.role.slice(1)
  return {
    id: member.user_id,
    membershipId: member.id,
    name: member.display_name,
    handle: member.email.split('@')[0] ?? member.email,
    email: member.email,
    role: role === 'Owner' || role === 'Admin' ? role : 'Member',
    color: `hsl(${hue} 55% 48%)`,
    online: false,
    title: '',
    roleIds: [],
    version: member.version,
  }
}

export function useWorkspaces() {
  return useQuery(workspacesQueryOptions())
}

export function useMembers(workspaceId: string | null) {
  return useQuery({
    queryKey: queryKeys.members(workspaceId ?? ''),
    enabled: Boolean(workspaceId),
    queryFn: async () => {
      const page = await fetchAllPages(async (cursor) => {
        const { data } = await listMembers({
          client: apiClient,
          path: { workspace_id: workspaceId! },
          query: { limit: 100, cursor },
          throwOnError: true,
        })
        return required(data, 'Members response was empty.')
      })
      return page.items.map(memberFromRecord)
    },
  })
}

export function useInvitations(workspaceId: string | null) {
  return useQuery({
    queryKey: queryKeys.invitations(workspaceId ?? ''),
    enabled: Boolean(workspaceId),
    queryFn: async () => {
      return fetchAllPages(async (cursor) => {
        const { data } = await listInvitations({
          client: apiClient,
          path: { workspace_id: workspaceId! },
          query: { limit: 100, cursor },
          throwOnError: true,
        })
        return required(data, 'Invitations response was empty.')
      })
    },
  })
}

export function useCreateWorkspace() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (name: string) => {
      const { data } = await createWorkspace({ client: apiClient, body: { name }, throwOnError: true })
      return required(data, 'Create workspace response was empty.')
    },
    onSuccess: (workspace) => {
      queryClient.setQueryData(queryKeys.workspaces, (items: unknown) =>
        Array.isArray(items) ? [...items, workspace] : [workspace])
    },
  })
}

export function useCreateInvitation(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: InvitationBody) => {
      const { data } = await createInvitation({
        client: apiClient,
        path: { workspace_id: workspaceId },
        body,
        throwOnError: true,
      })
      return required(data, 'Invitation response was empty.')
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.invitations(workspaceId) }),
  })
}

export function useChangeMemberRole(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (input: { membershipId: string; version: number; role: RoleBody }) => {
      await changeMemberRole({
        client: apiClient,
        path: { workspace_id: workspaceId, membership_id: input.membershipId },
        body: { expected_version: input.version, role: input.role },
        throwOnError: true,
      })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.members(workspaceId) }),
  })
}

export function useRemoveMember(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (input: { membershipId: string; version: number }) => {
      await removeMember({
        client: apiClient,
        path: { workspace_id: workspaceId, membership_id: input.membershipId },
        query: { expected_version: input.version },
        throwOnError: true,
      })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.members(workspaceId) }),
  })
}

export function useTransferOwnership(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (input: { membershipId: string; membershipVersion: number; workspaceVersion: number }) => {
      await transferOwnership({
        client: apiClient,
        path: { workspace_id: workspaceId },
        body: {
          membership_id: input.membershipId,
          membership_version: input.membershipVersion,
          expected_version: input.workspaceVersion,
        },
        throwOnError: true,
      })
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
      void queryClient.invalidateQueries({ queryKey: queryKeys.members(workspaceId) })
    },
  })
}

export function useRenameWorkspace(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (input: { name: string; version: number }) => {
      const { data } = await renameWorkspace({
        client: apiClient,
        path: { workspace_id: workspaceId },
        body: { expected_version: input.version, name: input.name },
        throwOnError: true,
      })
      return required(data, 'Rename workspace response was empty.')
    },
    onSuccess: (workspace) => queryClient.setQueryData(queryKeys.workspaces, (items: unknown) =>
      Array.isArray(items) ? items.map((item) => typeof item === 'object' && item && 'id' in item && item.id === workspaceId ? workspace : item) : items),
  })
}

export function useInvitationPreview(token: string | null) {
  return useQuery({
    queryKey: ['invitation-preview', token],
    enabled: !!token,
    retry: false,
    gcTime: 0,
    queryFn: async () => {
      const { data } = await previewInvitation({ client: apiClient, body: { token: token! }, throwOnError: true })
      return required(data, 'Invitation preview response was empty.')
    },
  })
}

export function useAcceptInvitation() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (body: AcceptBody) => {
      const { data } = await acceptInvitation({ client: apiClient, body, throwOnError: true })
      return required(data, 'Invitation acceptance response was empty.')
    },
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.currentUser }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      ])
    },
  })
}

export function useRevokeInvitation(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (invitationId: string) => {
      await revokeInvitation({
        client: apiClient,
        path: { workspace_id: workspaceId, invitation_id: invitationId },
        throwOnError: true,
      })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.invitations(workspaceId) }),
  })
}

export function useDeleteWorkspace(workspaceId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: async (version: number) => {
      await deleteWorkspace({
        client: apiClient,
        path: { workspace_id: workspaceId },
        query: { expected_version: version },
        throwOnError: true,
      })
    },
    onSuccess: async () => {
      await queryClient.cancelQueries({ queryKey: queryKeys.workspaces })
      await queryClient.cancelQueries({ queryKey: queryKeys.workspace(workspaceId) })
      queryClient.removeQueries({ queryKey: queryKeys.workspace(workspaceId) })
      queryClient.setQueryData<WorkspaceRecord[]>(queryKeys.workspaces, (items) =>
        items?.filter((item) => item.id !== workspaceId))
      if (window.localStorage.getItem('orbit:selected_workspace') === workspaceId) {
        window.localStorage.removeItem('orbit:selected_workspace')
      }
    },
  })
}
