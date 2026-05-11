import { AppShell, type Banner } from '@/components/layout/app-shell'
import { ThemeProvider } from '@/components/layout/theme-provider'
import { useConfig } from '@/hooks/useConfig'
import ChatView from '@/views/Chat'
import IngestView from '@/views/Ingest'
import SettingsView from '@/views/Settings'
import { useState } from 'react'
import { HashRouter, Route, Routes } from 'react-router-dom'

function AppRoutes() {
  const [banner, setBanner] = useState<Banner>(null)
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null)
  const { cfg } = useConfig()

  return (
    <Routes>
      <Route
        element={
          <AppShell
            banner={banner}
            onDismissBanner={() => setBanner(null)}
            activeSessionId={activeSessionId}
            onSessionSelect={setActiveSessionId}
          />
        }
      >
        <Route
          index
          element={
            <ChatView
              cfg={cfg}
              activeSessionId={activeSessionId}
              setActiveSessionId={setActiveSessionId}
              onBanner={setBanner}
            />
          }
        />
        <Route
          path="ingest"
          element={<IngestView cfg={cfg} onBanner={setBanner} />}
        />
        <Route
          path="settings"
          element={<SettingsView onBanner={setBanner} />}
        />
      </Route>
    </Routes>
  )
}

export default function App() {
  return (
    <ThemeProvider>
      <HashRouter>
        <AppRoutes />
      </HashRouter>
    </ThemeProvider>
  )
}
