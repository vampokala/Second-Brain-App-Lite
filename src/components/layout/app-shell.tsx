import { Toast } from '@/components/ui/toast'
import { useState } from 'react'
import { Outlet } from 'react-router-dom'
import { ModeToggle } from './mode-toggle'
import { Sidebar } from './sidebar'

export type Banner = { kind: 'success' | 'error'; text: string } | null

interface AppShellProps {
  banner: Banner
  onDismissBanner: () => void
  activeSessionId: string | null
  onSessionSelect: (id: string) => void
}

export function AppShell({ banner, onDismissBanner, activeSessionId, onSessionSelect }: AppShellProps) {
  const [collapsed, setCollapsed] = useState(() => {
    return localStorage.getItem('sidebar-collapsed') === 'true'
  })

  const toggleSidebar = () => {
    setCollapsed((prev) => {
      const next = !prev
      localStorage.setItem('sidebar-collapsed', String(next))
      return next
    })
  }

  return (
    <div className="flex h-screen overflow-hidden bg-[var(--color-background)]">
      <Sidebar
        collapsed={collapsed}
        onToggle={toggleSidebar}
        activeSessionId={activeSessionId}
        onSessionSelect={onSessionSelect}
      />

      <div className="flex flex-col flex-1 min-w-0 overflow-hidden">
        {/* Topbar */}
        <div className="flex items-center justify-end gap-2 px-4 py-2 border-b border-[var(--color-border)] shrink-0">
          <ModeToggle />
        </div>

        {/* Toast banner */}
        {banner && (
          <div className="px-4 pt-3">
            <Toast kind={banner.kind} message={banner.text} onDismiss={onDismissBanner} />
          </div>
        )}

        {/* Main content */}
        <main className="flex-1 overflow-auto">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
