import { useEffect } from 'react'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { Toaster } from '@/components/ui/sonner'
import { NewVersionNotice } from '@/app/NewVersionNotice'
import { Navigate, Outlet, Route, Routes, useLocation, useNavigate } from 'react-router'
import { setAppNavigate } from '@/lib/navigateBridge'
import { AppShell } from '@/app/shell/AppShell'
import { chatEnabled } from '@/app/shell/productNavigation'
import { ChatPage } from '@/features/chat/pages/ChatPage'
import { ProjectSettingsPage } from '@/features/tasks/pages/ProjectSettingsPage'
import { TasksPage } from '@/features/tasks/pages/TasksPage'
import { TaskTrashPage } from '@/features/tasks/pages/TaskTrashPage'
import { DocsPage } from '@/features/docs/pages/DocsPage'
import { AccountLayout, SettingsLayout } from '@/features/settings/pages/SettingsLayout'
import { GeneralPage } from '@/features/settings/pages/GeneralPage'
import { GithubPage } from '@/features/settings/pages/GithubPage'
import { LabelsPage } from '@/features/settings/pages/LabelsPage'
import { DangerZonePage } from '@/features/settings/pages/DangerZonePage'
import { MembersPage } from '@/features/settings/pages/MembersPage'
import { SessionsPage } from '@/features/settings/pages/SessionsPage'
import { ShortcutsPage } from '@/features/settings/pages/ShortcutsPage'
import { NotificationsPage } from '@/features/settings/pages/NotificationsPage'
import { ApiTokensPage } from '@/features/settings/pages/ApiTokensPage'
import { ProfilePage } from '@/features/profile/pages/ProfilePage'
import { InboxPage } from '@/features/inbox/pages/InboxPage'
import { AuthGate } from '@/features/auth/AuthGate'
import { AcceptInvitationPage } from '@/features/auth/pages/AcceptInvitationPage'
import { OAuthConsentPage } from '@/features/auth/pages/OAuthConsentPage'
import { LoginPage } from '@/features/auth/pages/LoginPage'
import { RecoveryPage } from '@/features/auth/pages/RecoveryPage'
import { SetupPage } from '@/features/auth/pages/SetupPage'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'
import { ViewsPage } from '@/features/views/ViewsPage'

/** Hands the router's navigate function to non-component code (markdown links). */
/** A page that has a new address: goes there and keeps the query and the hash. */
function MovedTo({ path }: { path: string }) {
  const { search, hash } = useLocation()
  return <Navigate to={{ pathname: path, search, hash }} replace />
}

function NavigateBridge() {
  const navigate = useNavigate()
  useEffect(() => {
    setAppNavigate((to) => void navigate(to))
  }, [navigate])
  return null
}

export default function App() {
  return (
    <>
      <NavigateBridge />
      <ConfirmationModalHost />
      <Toaster />
      <NewVersionNotice />
      <Routes>
        <Route path="setup" element={<SetupPage />} />
        <Route path="oauth/consent" element={<OAuthConsentPage />} />
        <Route path="login" element={<LoginPage />} />
        <Route path="recovery" element={<RecoveryPage />} />
        <Route path="accept-invitation" element={<AcceptInvitationPage />} />
        <Route element={<AuthGate />}>
          <Route element={<WorkspaceProvider><Outlet /></WorkspaceProvider>}>
            <Route element={<AppShell />}>
              <Route index element={<Navigate to="/tasks" replace />} />
              <Route path="tasks" element={<TasksPage />} />
              <Route path="tasks/projects/:projectId/settings" element={<ProjectSettingsPage />} />
              <Route path="tasks/:taskId" element={<TasksPage />} />
              <Route path="tasks-trash" element={<TaskTrashPage />} />
              <Route path="views" element={<ViewsPage />} />
              <Route path="views/:viewId" element={<TasksPage />} />
              <Route path="views/:viewId/:taskId" element={<TasksPage />} />
              <Route path="docs/trash" element={<DocsPage view="trash" />} />
              <Route path="docs/import/:importId?" element={<DocsPage view="import" />} />
              <Route path="docs/:pageId?" element={<DocsPage />} />
              <Route path="mail/*" element={<Navigate to="/tasks" replace />} />
              {chatEnabled ? (
                <>
                  <Route path="chat" element={<ChatPage />} />
                  <Route path="chat/unreads" element={<ChatPage />} />
                  <Route path="chat/threads" element={<ChatPage />} />
                  <Route path="chat/:conversationId" element={<ChatPage />} />
                  <Route path="chat/:conversationId/thread/:messageId" element={<ChatPage />} />
                </>
              ) : (
                <Route path="chat/*" element={<Navigate to="/tasks" replace />} />
              )}
              <Route path="dm/*" element={<Navigate to={chatEnabled ? '/chat' : '/tasks'} replace />} />
              <Route path="inbox" element={<InboxPage />} />
              <Route path="profile" element={<AccountLayout />}>
                <Route index element={<ProfilePage />} />
                <Route path="notifications" element={<NotificationsPage />} />
                <Route path="sessions" element={<SessionsPage />} />
                <Route path="shortcuts" element={<ShortcutsPage />} />
              </Route>
              <Route path="settings" element={<SettingsLayout />}>
                <Route index element={<GeneralPage />} />
                <Route path="labels" element={<LabelsPage />} />
                <Route path="github" element={<GithubPage />} />
                <Route path="danger-zone" element={<DangerZonePage />} />
                <Route path="members" element={<MembersPage />} />
                <Route path="invitations" element={<Navigate to="/settings/members" replace />} />
                {/* these moved to the account settings; old links and bookmarks still work */}
                <Route path="sessions" element={<MovedTo path="/profile/sessions" />} />
                <Route path="shortcuts" element={<MovedTo path="/profile/shortcuts" />} />
                <Route path="notifications" element={<MovedTo path="/profile/notifications" />} />
                <Route path="api-tokens" element={<ApiTokensPage />} />
              </Route>
              <Route path="*" element={<Navigate to="/tasks" replace />} />
            </Route>
          </Route>
        </Route>
      </Routes>
    </>
  )
}
