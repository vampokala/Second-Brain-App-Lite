import { Button } from '@/components/ui/button'
import { CopyButton } from '@/components/ui/copy-button'
import { Textarea } from '@/components/ui/textarea'
import { chatClipboardText, useChatSession } from '@/hooks/useChatSession'
import { cn } from '@/lib/utils'
import type { AppConfig } from '@/types'
import { BookmarkPlus, BrainCircuit, Loader2, Send } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'

type Banner = { kind: 'success' | 'error'; text: string } | null

function Message({ role, content, isStreaming }: { role: string; content: string; isStreaming?: boolean }) {
  const isAssistant = role === 'assistant'
  return (
    <div className="group flex flex-col gap-1.5 py-4 border-b border-[var(--color-border)] last:border-0">
      <div className="flex items-center justify-between gap-2">
        <span className={cn(
          'text-[10px] font-semibold uppercase tracking-widest',
          isAssistant ? 'text-[var(--color-accent)]' : 'text-[var(--color-muted-foreground)]',
        )}>
          {role}{isStreaming ? ' ···' : ''}
        </span>
        <div className="opacity-0 group-hover:opacity-100 transition-opacity">
          <CopyButton text={chatClipboardText(content)} label={`Copy ${role} message`} />
        </div>
      </div>
      <div className={cn('text-sm leading-relaxed text-[var(--color-foreground)]', isAssistant && 'prose')}>
        {isAssistant ? <ReactMarkdown>{content}</ReactMarkdown> : <span className="whitespace-pre-wrap">{content}</span>}
      </div>
    </div>
  )
}

interface ChatViewProps {
  cfg: AppConfig | null
  activeSessionId: string | null
  setActiveSessionId: (id: string) => void
  onBanner: (b: Banner) => void
}

export default function ChatView({ cfg, activeSessionId, setActiveSessionId, onBanner }: ChatViewProps) {
  const {
    activeSession,
    composer, setComposer,
    sendBusy, streamTail,
    saveTitle, setSaveTitle,
    sendChat, saveLastToWiki, rollMemory,
  } = useChatSession(activeSessionId, setActiveSessionId)

  const [showSaveBar, setShowSaveBar] = useState(false)
  const messagesEndRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [activeSession?.messages.length, streamTail])

  const handleSend = async () => {
    if (!cfg) return
    try {
      await sendChat(cfg)
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const handleSaveToWiki = async () => {
    if (!cfg) return
    try {
      const path = await saveLastToWiki(cfg)
      onBanner({ kind: 'success', text: `Saved wiki/${path}` })
      setShowSaveBar(false)
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const handleRollMemory = async () => {
    if (!cfg) return
    try {
      await rollMemory(cfg)
      onBanner({ kind: 'success', text: 'Rolling memory updated.' })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault()
      handleSend()
    }
  }

  if (!cfg) return <div className="flex items-center justify-center h-full text-[var(--color-muted-foreground)] text-sm">Loading…</div>

  if (!activeSessionId) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3 text-center px-6">
        <p className="text-[var(--color-muted-foreground)] text-sm">No session selected.</p>
        <p className="text-xs text-[var(--color-muted-foreground)]">Use the + button in the sidebar to start a new chat.</p>
      </div>
    )
  }

  const hasMessages = (activeSession?.messages.length ?? 0) > 0 || streamTail

  return (
    <div className="flex flex-col h-full max-w-3xl mx-auto w-full px-4">
      {/* Session title */}
      <div className="py-3 border-b border-[var(--color-border)] shrink-0">
        <p className="text-sm font-medium text-[var(--color-foreground)]">{activeSession?.title ?? 'Chat'}</p>
        <p className="text-xs text-[var(--color-muted-foreground)] mt-0.5">Provider: {cfg.defaultProvider}</p>
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto py-2 min-h-0">
        {!hasMessages && (
          <div className="flex flex-col items-center justify-center h-full gap-2 text-center py-12">
            <BrainCircuit size={32} className="text-[var(--color-muted-foreground)] opacity-40" />
            <p className="text-sm text-[var(--color-muted-foreground)]">Ask something grounded in your wiki…</p>
          </div>
        )}
        {activeSession?.messages.map((m, i) => (
          <Message key={`${m.ts ?? ''}-${i}`} role={m.role} content={m.content} />
        ))}
        {streamTail && (
          <Message role="assistant" content={streamTail} isStreaming />
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Save bar (contextual) */}
      {showSaveBar && (
        <div className="shrink-0 border-t border-[var(--color-border)] py-2 flex gap-2 items-center">
          <input
            type="text"
            className="flex-1 h-8 px-3 text-sm rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-background)] text-[var(--color-foreground)] focus:outline-2 focus:outline-[var(--color-accent)] focus:outline-offset-0"
            value={saveTitle}
            onChange={(e) => setSaveTitle(e.target.value)}
            placeholder="Title for wiki entry"
          />
          <Button size="sm" onClick={handleSaveToWiki}>Save to wiki</Button>
          <Button variant="ghost" size="sm" onClick={() => setShowSaveBar(false)}>Cancel</Button>
        </div>
      )}

      {/* Composer */}
      <div className="shrink-0 border-t border-[var(--color-border)] py-3 flex flex-col gap-2">
        <Textarea
          value={composer}
          onChange={(e) => setComposer(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Ask something… (⌘↵ to send)"
          disabled={sendBusy}
          className="min-h-20 resize-none"
        />
        <div className="flex items-center gap-2 justify-between">
          <div className="flex items-center gap-1.5">
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setShowSaveBar(!showSaveBar)}
              title="Save last answer to wiki"
              className="gap-1.5"
            >
              <BookmarkPlus size={14} />
              Save to wiki
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={handleRollMemory}
              title="Update rolling memory"
              className="gap-1.5"
            >
              <BrainCircuit size={14} />
              Update memory
            </Button>
          </div>
          <Button
            onClick={handleSend}
            disabled={sendBusy || !composer.trim()}
            size="sm"
            className="gap-1.5"
          >
            {sendBusy ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
            Send
          </Button>
        </div>
      </div>
    </div>
  )
}
