import { useEffect } from 'react'
import { Navigate, Outlet, Route, Routes, useNavigate } from 'react-router'
import { setAppNavigate } from './lib/navigateBridge'
import { AppShell } from './components/shell/AppShell'
import { HomePage } from './features/home/HomePage'
import { ProjectSettingsPage } from './features/tasks/ProjectSettingsPage'
import { TasksPage } from './features/tasks/TasksPage'
import { TaskTrashPage } from './features/tasks/TaskTrashPage'
import { DocsPage } from './features/docs/DocsPage'
import { MailPage } from './features/mail/MailPage'
import { ChatPage } from './features/chat/ChatPage'
import { DMPage } from './features/chat/DMPage'
import { InboxPage } from './features/inbox/InboxPage'
import { SettingsLayout } from './features/settings/SettingsLayout'
import { GeneralPage } from './features/settings/GeneralPage'
import { MembersPage } from './features/settings/MembersPage'
import { SessionsPage } from './features/settings/SessionsPage'
import { ProfilePage } from './features/profile/ProfilePage'
import { ServerSettingsPage } from './features/chat/ServerSettingsPage'
import { AcceptInvitationPage, AuthGate, LoginPage, RecoveryPage, SetupPage } from './features/auth/AuthGate'
import { WorkspaceProvider } from './features/workspaces/WorkspaceProvider'
import './features/auth/auth.css'

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
      <Routes>
        <Route path="setup" element={<SetupPage />} />
        <Route path="login" element={<LoginPage />} />
        <Route path="recovery" element={<RecoveryPage />} />
        <Route path="accept-invitation" element={<AcceptInvitationPage />} />
        <Route element={<AuthGate />}>
          <Route element={<WorkspaceProvider><Outlet /></WorkspaceProvider>}>
            <Route element={<AppShell />}>
              <Route index element={<HomePage />} />
              <Route path="tasks" element={<TasksPage />} />
              <Route path="tasks/projects/:projectId/settings" element={<ProjectSettingsPage />} />
              <Route path="tasks/:taskId" element={<TasksPage />} />
              <Route path="tasks-trash" element={<TaskTrashPage />} />
              <Route path="docs" element={<DocsPage />} />
              <Route path="docs/:docId" element={<DocsPage />} />
              <Route path="mail" element={<MailPage />} />
              <Route path="mail/:threadId" element={<MailPage />} />
              <Route path="chat" element={<ChatPage />} />
              <Route path="chat/settings" element={<ServerSettingsPage />} />
              <Route path="chat/:channelId" element={<ChatPage />} />
              <Route path="chat/:channelId/thread/:rootId" element={<ChatPage />} />
              <Route path="dm" element={<DMPage />} />
              <Route path="dm/:dmId" element={<DMPage />} />
              <Route path="inbox" element={<InboxPage />} />
              <Route path="profile" element={<ProfilePage />} />
              <Route path="settings" element={<SettingsLayout />}>
                <Route index element={<GeneralPage />} />
                <Route path="members" element={<MembersPage />} />
                <Route path="sessions" element={<SessionsPage />} />
              </Route>
              <Route path="*" element={<Navigate to="/" replace />} />
            </Route>
          </Route>
        </Route>
      </Routes>
    </>
  )
}
