import { BrowserRouter, Navigate, Route, Routes } from 'react-router'
import { ThemeProvider } from './lib/theme'
import { AppShell } from './components/shell/AppShell'
import { HomePage } from './features/home/HomePage'
import { TasksPage } from './features/tasks/TasksPage'
import { DocsPage } from './features/docs/DocsPage'
import { MailPage } from './features/mail/MailPage'
import { ChatPage } from './features/chat/ChatPage'
import { InboxPage } from './features/inbox/InboxPage'
import { SettingsPage } from './features/settings/SettingsPage'

export default function App() {
  return (
    <ThemeProvider>
      <BrowserRouter>
        <Routes>
          <Route element={<AppShell />}>
            <Route index element={<HomePage />} />
            <Route path="tasks" element={<TasksPage />} />
            <Route path="tasks/:taskId" element={<TasksPage />} />
            <Route path="docs" element={<DocsPage />} />
            <Route path="docs/:docId" element={<DocsPage />} />
            <Route path="mail" element={<MailPage />} />
            <Route path="mail/:threadId" element={<MailPage />} />
            <Route path="chat" element={<ChatPage />} />
            <Route path="chat/:channelId" element={<ChatPage />} />
            <Route path="inbox" element={<InboxPage />} />
            <Route path="settings" element={<SettingsPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </ThemeProvider>
  )
}
