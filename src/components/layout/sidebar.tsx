import { cn } from '@/lib/utils'
import { invoke } from '@tauri-apps/api/core'
import { Brain, ChevronDown, ChevronLeft, ChevronRight, MessageSquare, Plus, Settings, Trash2, Upload } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { NavLink, useNavigate } from 'react-router-dom'
import type { SessionFile } from '@/types'

function NavItem({ to, icon: Icon, label }: { to: string; icon: React.ElementType; label: string }) {
  return (
    <NavLink
      to={to}
      className={({ isActive }) =>
        cn(
          'flex items-center gap-2.5 px-3 py-1.5 rounded-[var(--radius-md)] text-sm transition-colors',
          isActive
            ? 'bg-[var(--color-muted)] text-[var(--color-foreground)] font-medium'
            : 'text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] hover:bg-[var(--color-muted)]',
        )
      }
    >
      <Icon size={16} />
      <span>{label}</span>
    </NavLink>
  )
}

interface SidebarProps {
  collapsed: boolean
  onToggle: () => void
  activeSessionId: string | null
  onSessionSelect: (id: string) => void
}

export function Sidebar({ collapsed, onToggle, activeSessionId, onSessionSelect }: SidebarProps) {
  const [sessions, setSessions] = useState<SessionFile[]>([])
  const [sessionsOpen, setSessionsOpen] = useState(true)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const navigate = useNavigate()
  const hasFetchedRef = useRef(false)

  useEffect(() => {
    if (hasFetchedRef.current) return
    hasFetchedRef.current = true
    invoke<SessionFile[]>('list_chat_sessions')
      .then(setSessions)
      .catch(() => setSessions([]))
  }, [])

  const newSession = async () => {
    try {
      const s = await invoke<SessionFile>('new_chat_session')
      setSessions((prev) => [s, ...prev])
      onSessionSelect(s.id)
      navigate('/')
    } catch {
      /* ignore */
    }
  }

  const deleteSession = async (id: string, e: React.MouseEvent) => {
    e.stopPropagation()
    setDeletingId(id)
    try {
      await invoke('delete_chat_session', { id })
      setSessions((prev) => prev.filter((s) => s.id !== id))
      if (activeSessionId === id) {
        const remaining = sessions.filter((s) => s.id !== id)
        onSessionSelect(remaining[0]?.id ?? '')
      }
    } catch {
      /* ignore */
    } finally {
      setDeletingId(null)
    }
  }

  if (collapsed) {
    return (
      <div className="flex flex-col h-full w-12 border-r border-[var(--color-sidebar-border)] bg-[var(--color-sidebar)] py-3 items-center gap-1">
        <button
          type="button"
          onClick={onToggle}
          title="Expand sidebar"
          className="flex items-center justify-center h-7 w-7 rounded-[var(--radius-md)] text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] hover:bg-[var(--color-muted)] transition-colors mb-2"
        >
          <ChevronRight size={16} />
        </button>
        <NavLink to="/" title="Chat" className={({ isActive }) => cn('flex items-center justify-center h-7 w-7 rounded-[var(--radius-md)] transition-colors', isActive ? 'bg-[var(--color-muted)] text-[var(--color-foreground)]' : 'text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] hover:bg-[var(--color-muted)]')}>
          <MessageSquare size={16} />
        </NavLink>
        <NavLink to="/ingest" title="Ingest" className={({ isActive }) => cn('flex items-center justify-center h-7 w-7 rounded-[var(--radius-md)] transition-colors', isActive ? 'bg-[var(--color-muted)] text-[var(--color-foreground)]' : 'text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] hover:bg-[var(--color-muted)]')}>
          <Upload size={16} />
        </NavLink>
        <div className="flex-1" />
        <NavLink to="/settings" title="Settings" className={({ isActive }) => cn('flex items-center justify-center h-7 w-7 rounded-[var(--radius-md)] transition-colors', isActive ? 'bg-[var(--color-muted)] text-[var(--color-foreground)]' : 'text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] hover:bg-[var(--color-muted)]')}>
          <Settings size={16} />
        </NavLink>
      </div>
    )
  }

  return (
    <div className="flex flex-col h-full w-60 border-r border-[var(--color-sidebar-border)] bg-[var(--color-sidebar)]">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-[var(--color-sidebar-border)]">
        <div className="flex items-center gap-2">
          <Brain size={18} className="text-[var(--color-accent)]" />
          <span className="text-sm font-semibold text-[var(--color-foreground)]">Second Brain</span>
        </div>
        <button
          type="button"
          onClick={onToggle}
          title="Collapse sidebar"
          className="flex items-center justify-center h-6 w-6 rounded-[var(--radius-sm)] text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] hover:bg-[var(--color-muted)] transition-colors"
        >
          <ChevronLeft size={14} />
        </button>
      </div>

      {/* Nav */}
      <div className="px-2 pt-3 pb-1 flex flex-col gap-0.5">
        <NavItem to="/" icon={MessageSquare} label="Chat" />
        <NavItem to="/ingest" icon={Upload} label="Ingest" />
      </div>

      {/* Sessions */}
      <div className="px-2 pt-3 flex-1 overflow-hidden flex flex-col min-h-0">
        {/* Section header — click label to collapse, + to create */}
        <div className="flex items-center justify-between px-1 mb-1.5">
          <button
            type="button"
            onClick={() => setSessionsOpen((o) => !o)}
            className="flex items-center gap-1 text-[10px] font-semibold uppercase tracking-widest text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] transition-colors"
          >
            <ChevronDown
              size={11}
              className={cn('transition-transform duration-150', !sessionsOpen && '-rotate-90')}
            />
            Sessions
          </button>
          <button
            type="button"
            onClick={newSession}
            title="New chat"
            className="flex items-center justify-center h-5 w-5 rounded-[var(--radius-sm)] text-[var(--color-muted-foreground)] hover:text-[var(--color-foreground)] hover:bg-[var(--color-muted)] transition-colors"
          >
            <Plus size={13} />
          </button>
        </div>

        {sessionsOpen && (
          <div className="flex-1 overflow-y-auto flex flex-col gap-0.5 pr-0.5">
            {sessions.length === 0 && (
              <p className="text-xs text-[var(--color-muted-foreground)] px-1 py-1">No sessions yet.</p>
            )}
            {sessions.map((s) => (
              <div
                key={s.id}
                className={cn(
                  'group flex items-center rounded-[var(--radius-md)] transition-colors',
                  s.id === activeSessionId
                    ? 'bg-[var(--color-muted)]'
                    : 'hover:bg-[var(--color-muted)]',
                )}
              >
                <button
                  type="button"
                  onClick={() => { onSessionSelect(s.id); navigate('/') }}
                  className={cn(
                    'flex-1 text-left px-2 py-1.5 text-xs truncate transition-colors min-w-0',
                    s.id === activeSessionId
                      ? 'text-[var(--color-foreground)] font-medium'
                      : 'text-[var(--color-muted-foreground)] group-hover:text-[var(--color-foreground)]',
                  )}
                >
                  {s.title}
                </button>
                <button
                  type="button"
                  onClick={(e) => deleteSession(s.id, e)}
                  disabled={deletingId === s.id}
                  title="Delete session"
                  className={cn(
                    'shrink-0 flex items-center justify-center h-5 w-5 mr-1 rounded-[var(--radius-sm)]',
                    'text-[var(--color-muted-foreground)] hover:text-[var(--color-destructive)] hover:bg-[color-mix(in_srgb,var(--color-destructive)_10%,transparent)]',
                    'opacity-0 group-hover:opacity-100 transition-opacity',
                    'disabled:cursor-not-allowed',
                  )}
                >
                  <Trash2 size={11} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="px-2 py-2 border-t border-[var(--color-sidebar-border)]">
        <NavItem to="/settings" icon={Settings} label="Settings" />
      </div>
    </div>
  )
}

export function useSessions() {
  const [sessions, setSessions] = useState<SessionFile[]>([])

  const refresh = async () => {
    try {
      const list = await invoke<SessionFile[]>('list_chat_sessions')
      setSessions(list)
    } catch {
      setSessions([])
    }
  }

  return { sessions, setSessions, refresh }
}
