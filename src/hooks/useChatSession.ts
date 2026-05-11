import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useRef, useState } from 'react'
import type { AppConfig, SessionFile } from '@/types'

export function useChatSession(activeSessionId: string | null, setActiveSessionId: (id: string) => void) {
  const [sessions, setSessions] = useState<SessionFile[]>([])
  const [composer, setComposer] = useState('')
  const [sendBusy, setSendBusy] = useState(false)
  const [streamTail, setStreamTail] = useState('')
  const [saveTitle, setSaveTitle] = useState('Chat insight')
  const hasFetchedRef = useRef(false)

  const refreshSessions = useCallback(async () => {
    try {
      const list = await invoke<SessionFile[]>('list_chat_sessions')
      setSessions(list)
      if (!activeSessionId && list.length) setActiveSessionId(list[0].id)
    } catch {
      setSessions([])
    }
  }, [activeSessionId, setActiveSessionId])

  useEffect(() => {
    if (hasFetchedRef.current) return
    hasFetchedRef.current = true
    refreshSessions()
  }, [refreshSessions])

  const newSession = async (): Promise<SessionFile> => {
    const s = await invoke<SessionFile>('new_chat_session')
    setSessions((prev) => [s, ...prev])
    setActiveSessionId(s.id)
    setStreamTail('')
    return s
  }

  const activeSession = sessions.find((s) => s.id === activeSessionId) ?? null

  const sendChat = async (cfg: AppConfig): Promise<void> => {
    if (!cfg || !activeSessionId || !composer.trim()) return
    setSendBusy(true)
    setStreamTail('')
    const userMessage = composer.trim()
    setComposer('')
    const un = await listen<string>('chat-token', (ev) => {
      setStreamTail((t) => t + ev.payload)
    })
    try {
      await invoke('chat_stream_cmd', { cfg, payload: { sessionId: activeSessionId, userMessage } })
      await refreshSessions()
    } finally {
      un()
      setSendBusy(false)
      setStreamTail('')
    }
  }

  const saveLastToWiki = async (cfg: AppConfig): Promise<string> => {
    const last = [...(activeSession?.messages ?? [])].reverse().find((m) => m.role === 'assistant')
    if (!last?.content) throw new Error('No assistant message to save.')
    return invoke<string>('save_answer_to_wiki', {
      cfg,
      args: { title: saveTitle.trim() || 'Chat insight', bodyMarkdown: last.content },
    })
  }

  const rollMemory = async (cfg: AppConfig): Promise<void> => {
    if (!activeSessionId) throw new Error('No active session.')
    return invoke('update_memory_roll_up', { cfg, sessionId: activeSessionId })
  }

  return {
    sessions, setSessions, refreshSessions,
    newSession, activeSession,
    composer, setComposer,
    sendBusy, streamTail,
    saveTitle, setSaveTitle,
    sendChat, saveLastToWiki, rollMemory,
  }
}

export function chatClipboardText(raw: string): string {
  const marker = '__ERROR__'
  const i = raw.indexOf(marker)
  if (i >= 0) return raw.slice(i + marker.length).trim()
  return raw
}
