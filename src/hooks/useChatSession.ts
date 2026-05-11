import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useRef, useState } from 'react'
import type { AppConfig, ChatRetrievalMeta, SessionFile } from '@/types'

export function useChatSession(activeSessionId: string | null, setActiveSessionId: (id: string) => void) {
  const [sessions, setSessions] = useState<SessionFile[]>([])
  const [composer, setComposer] = useState('')
  const [sendBusy, setSendBusy] = useState(false)
  const [streamTail, setStreamTail] = useState('')
  const [saveTitle, setSaveTitle] = useState('Chat insight')
  const [wikiSourcesOnly, setWikiSourcesOnly] = useState(true)
  const [includeWebSearch, setIncludeWebSearch] = useState(false)
  const [braveKeyConfigured, setBraveKeyConfigured] = useState(false)
  const [lastRetrievalMeta, setLastRetrievalMeta] = useState<ChatRetrievalMeta | null>(null)
  const hasFetchedRef = useRef(false)

  const refreshBraveHint = useCallback(async () => {
    try {
      const h = await invoke<string | null>('api_secret_hint', { provider: 'brave' })
      setBraveKeyConfigured(Boolean(h?.trim()))
    } catch {
      setBraveKeyConfigured(false)
    }
  }, [])

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
    void refreshBraveHint()
  }, [refreshSessions, refreshBraveHint])

  useEffect(() => {
    setLastRetrievalMeta(null)
  }, [activeSessionId])

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
    const unToken = await listen<string>('chat-token', (ev) => {
      setStreamTail((t) => t + ev.payload)
    })
    const unMeta = await listen<ChatRetrievalMeta>('chat-retrieval-meta', (ev) => {
      setLastRetrievalMeta(ev.payload)
      if (ev.payload.braveKeyConfigured !== undefined) {
        setBraveKeyConfigured(ev.payload.braveKeyConfigured)
      }
    })
    try {
      await invoke('chat_stream_cmd', {
        cfg,
        payload: {
          sessionId: activeSessionId,
          userMessage,
          wikiSourcesOnly,
          includeWebSearch,
        },
      })
      await refreshSessions()
    } finally {
      unToken()
      unMeta()
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
    wikiSourcesOnly, setWikiSourcesOnly,
    includeWebSearch, setIncludeWebSearch,
    braveKeyConfigured, refreshBraveHint,
    lastRetrievalMeta,
    sendChat, saveLastToWiki, rollMemory,
  }
}

export function chatClipboardText(raw: string): string {
  const marker = '__ERROR__'
  const i = raw.indexOf(marker)
  if (i >= 0) return raw.slice(i + marker.length).trim()
  return raw
}
