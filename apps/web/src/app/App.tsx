import { ConnectionScreen } from './ConnectionScreen'
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
import { ProjectsPage } from '@/features/tasks/pages/ProjectsPage'
import { ProjectOverviewPage } from '@/features/tasks/pages/ProjectOverviewPage'
import { MilestonePage } from '@/features/tasks/pages/MilestonePage'
import { RoadmapPage } from '@/features/tasks/pages/RoadmapPage'
import { TriagePage } from '@/features/tasks/pages/TriagePage'
import { CyclePage, CyclesPage } from '@/features/tasks/pages/CyclesPage'
import { InsightsPage } from '@/features/tasks/pages/InsightsPage'
import { TasksPage } from '@/features/tasks/pages/TasksPage'
import { HomePage } from '@/features/home/pages/HomePage'
import { TaskTrashPage } from '@/features/tasks/pages/TaskTrashPage'
import { DocsPage } from '@/features/docs/pages/DocsPage'
import { AccountLayout, AdminLayout, SettingsLayout } from '@/features/settings/pages/SettingsLayout'
import { AdminAuditPage } from '@/features/admin/pages/AdminAuditPage'
import { AdminBackupsPage } from '@/features/admin/pages/AdminBackupsPage'
import { AdminStoragePage } from '@/features/admin/pages/AdminStoragePage'
import { AdminSettingsPage } from '@/features/admin/pages/AdminSettingsPage'
import { AdminUsersPage } from '@/features/admin/pages/AdminUsersPage'
import { GeneralPage } from '@/features/settings/pages/GeneralPage'
import { GithubPage } from '@/features/settings/pages/GithubPage'
import { EmojiPage } from '@/features/settings/pages/EmojiPage'
import { LabelsPage } from '@/features/settings/pages/LabelsPage'
import { DangerZonePage } from '@/features/settings/pages/DangerZonePage'
import { MembersPage } from '@/features/settings/pages/MembersPage'
import { SecurityPage } from '@/features/settings/pages/SecurityPage'
import { SessionsPage } from '@/features/settings/pages/SessionsPage'
import { ShortcutsPage } from '@/features/settings/pages/ShortcutsPage'
import { StickersPage } from '@/features/settings/pages/StickersPage'
import { StoragePage } from '@/features/settings/pages/StoragePage'
import { NotificationsPage } from '@/features/settings/pages/NotificationsPage'
import { ApiTokensPage } from '@/features/settings/pages/ApiTokensPage'
import { ProfilePage } from '@/features/profile/pages/ProfilePage'
import { InboxPage } from '@/features/inbox/pages/InboxPage'
import { AuthGate, GuestGate } from '@/features/auth/AuthGate'
import { AcceptInvitationPage } from '@/features/auth/pages/AcceptInvitationPage'
import { OAuthConsentPage } from '@/features/auth/pages/OAuthConsentPage'
import { LoginPage } from '@/features/auth/pages/LoginPage'
import { RecoveryPage } from '@/features/auth/pages/RecoveryPage'
import { RegisterPage } from '@/features/auth/pages/RegisterPage'
import { SetupPage } from '@/features/auth/pages/SetupPage'
import { OnboardingPage } from '@/features/onboarding/OnboardingPage'
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
      <ConnectionScreen />
      <Routes>
        <Route element={<GuestGate />}>
          <Route path="setup" element={<SetupPage />} />
          <Route path="login" element={<LoginPage />} />
          <Route path="recovery" element={<RecoveryPage />} />
          <Route path="register" element={<RegisterPage />} />
        </Route>
        <Route path="oauth/consent" element={<OAuthConsentPage />} />
        <Route path="accept-invitation" element={<AcceptInvitationPage />} />
        <Route element={<AuthGate />}>
          <Route path="onboarding" element={<OnboardingPage />} />
          <Route element={<WorkspaceProvider><Outlet /></WorkspaceProvider>}>
            <Route element={<AppShell />}>
              <Route index element={<HomePage />} />
              <Route path="tasks" element={<TasksPage />} />
              <Route path="tasks/projects" element={<ProjectsPage />} />
              <Route path="tasks/roadmap" element={<RoadmapPage />} />
              <Route path="tasks/triage" element={<TriagePage />} />
              <Route path="tasks/projects/:projectId" element={<ProjectOverviewPage />} />
              <Route path="tasks/projects/:projectId/settings" element={<ProjectSettingsPage />} />
              <Route path="tasks/projects/:projectId/milestones/:milestoneId" element={<MilestonePage />} />
              <Route path="tasks/projects/:projectId/insights" element={<InsightsPage />} />
              <Route path="tasks/projects/:projectId/cycles" element={<CyclesPage />} />
              <Route path="tasks/projects/:projectId/cycles/:cycleId" element={<CyclePage />} />
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
              <Route path="activity" element={<InboxPage />} />
              <Route path="inbox" element={<MovedTo path="/activity" />} />
              <Route path="profile" element={<AccountLayout />}>
                <Route index element={<ProfilePage />} />
                <Route path="notifications" element={<NotificationsPage />} />
                <Route path="security" element={<SecurityPage />} />
                <Route path="sessions" element={<SessionsPage />} />
                <Route path="shortcuts" element={<ShortcutsPage />} />
                <Route path="storage" element={<StoragePage />} />
              </Route>
              <Route path="admin" element={<AdminLayout />}>
                <Route index element={<AdminSettingsPage />} />
                <Route path="users" element={<AdminUsersPage />} />
                <Route path="audit" element={<AdminAuditPage />} />
                <Route path="storage" element={<AdminStoragePage />} />
                <Route path="backups" element={<AdminBackupsPage />} />
              </Route>
              <Route path="settings" element={<SettingsLayout />}>
                <Route index element={<GeneralPage />} />
                <Route path="labels" element={<LabelsPage />} />
                <Route path="emoji" element={<EmojiPage />} />
                <Route path="stickers" element={<StickersPage />} />
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
