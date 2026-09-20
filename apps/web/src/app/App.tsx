import { useEffect } from 'react'
import { ConfirmationModalHost } from '@/components/common/ConfirmationModal'
import { Toaster } from '@/components/ui/sonner'
import { Navigate, Outlet, Route, Routes, useNavigate } from 'react-router'
import { setAppNavigate } from '@/lib/navigateBridge'
import { AppShell } from '@/app/shell/AppShell'
import { ProjectSettingsPage } from '@/features/tasks/pages/ProjectSettingsPage'
import { TasksPage } from '@/features/tasks/pages/TasksPage'
import { TaskTrashPage } from '@/features/tasks/pages/TaskTrashPage'
import { SettingsLayout } from '@/features/settings/pages/SettingsLayout'
import { GeneralPage } from '@/features/settings/pages/GeneralPage'
import { DangerZonePage } from '@/features/settings/pages/DangerZonePage'
import { MembersPage } from '@/features/settings/pages/MembersPage'
import { SessionsPage } from '@/features/settings/pages/SessionsPage'
import { ApiTokensPage } from '@/features/settings/pages/ApiTokensPage'
import { ProfilePage } from '@/features/profile/pages/ProfilePage'
import { InboxPage } from '@/features/inbox/pages/InboxPage'
import { AuthGate } from '@/features/auth/AuthGate'
import { AcceptInvitationPage } from '@/features/auth/pages/AcceptInvitationPage'
import { LoginPage } from '@/features/auth/pages/LoginPage'
import { RecoveryPage } from '@/features/auth/pages/RecoveryPage'
import { SetupPage } from '@/features/auth/pages/SetupPage'
import { WorkspaceProvider } from '@/features/workspaces/WorkspaceProvider'

/** Hands the router's navigate function to non-component code (markdown links). */
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
      <Routes>
        <Route path="setup" element={<SetupPage />} />
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
              <Route path="docs/*" element={<Navigate to="/tasks" replace />} />
              <Route path="mail/*" element={<Navigate to="/tasks" replace />} />
              <Route path="chat/*" element={<Navigate to="/tasks" replace />} />
              <Route path="dm/*" element={<Navigate to="/tasks" replace />} />
              <Route path="inbox" element={<InboxPage />} />
              <Route path="profile" element={<ProfilePage />} />
              <Route path="settings" element={<SettingsLayout />}>
                <Route index element={<GeneralPage />} />
                <Route path="danger-zone" element={<DangerZonePage />} />
                <Route path="members" element={<MembersPage />} />
                <Route path="invitations" element={<Navigate to="/settings/members" replace />} />
                <Route path="sessions" element={<SessionsPage />} />
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
