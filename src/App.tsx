import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useState } from 'react'
import ReactMarkdown from 'react-markdown'

type AppConfig = {
  osHint: string
  rawDir?: string | null
  wikiDir?: string | null
  schemaDir?: string | null
  vaultRoot?: string | null
  defaultProvider: string
  ollamaEnabled: boolean
  ollamaBaseUrl: string
  ollamaModel: string
  openaiModel: string
  anthropicModel: string
  geminiBaseUrl: string
  geminiModel: string
  compatibleBaseUrl: string
  compatibleModel: string
  theme: string
}

type SessionFile = {
  id: string
  title: string
  created: string
  updated: string
  messages: { role: string; content: string; ts?: string }[]
}

type FileIngestResult = {
  relativeRawPath: string
  status: string
  detail?: string | null
}

type IngestProgressPayload = {
  phase: string
  message: string
  current?: number
  total?: number
  relativePath?: string
}

function formatIngestProgressLine(p: IngestProgressPayload): string {
  const step = p.current != null && p.total != null ? `[${p.current}/${p.total}] ` : ''
  const path = p.relativePath ? `${p.relativePath} · ` : ''
  return `${step}${path}${p.phase}: ${p.message}`
}

type SchemaStatus = {
  claudeMd: boolean
  llmWikiMd: boolean
}

/** Removes streaming error sentinel so clipboard gets the readable error text. */
function chatClipboardText(raw: string): string {
  const marker = '__ERROR__'
  const i = raw.indexOf(marker)
  if (i >= 0) return raw.slice(i + marker.length).trim()
  return raw
}

async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    try {
      const ta = document.createElement('textarea')
      ta.value = text
      ta.style.position = 'fixed'
      ta.style.left = '-9999px'
      document.body.appendChild(ta)
      ta.select()
      document.execCommand('copy')
      document.body.removeChild(ta)
      return true
    } catch {
      return false
    }
  }
}

function CopyIconButton({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false)
  const disabled = !text.trim().length
  return (
    <button
      type="button"
      className="icon-copy-btn"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={async () => {
        if (disabled) return
        const ok = await copyToClipboard(text)
        if (ok) {
          setCopied(true)
          window.setTimeout(() => setCopied(false), 1600)
        }
      }}
    >
      {copied ? (
        <span aria-hidden="true">✓</span>
      ) : (
        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
          <rect x="9" y="9" width="13" height="13" rx="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        </svg>
      )}
    </button>
  )
}

async function loadCfg(): Promise<AppConfig> {
  return invoke<AppConfig>('load_app_config')
}

async function saveCfg(cfg: AppConfig): Promise<void> {
  return invoke('save_app_config', { cfg })
}

export default function App() {
  const [tab, setTab] = useState<'configuration' | 'ingest' | 'chat'>('configuration')
  const [cfg, setCfg] = useState<AppConfig | null>(null)
  const [banner, setBanner] = useState<{ kind: 'error' | 'success'; text: string } | null>(null)
  const [schemaStatus, setSchemaStatus] = useState<SchemaStatus | null>(null)
  const [platformOs, setPlatformOs] = useState<string>('')

  const [openaiSecretInput, setOpenaiSecretInput] = useState('')
  const [anthropicSecretInput, setAnthropicSecretInput] = useState('')
  const [geminiSecretInput, setGeminiSecretInput] = useState('')
  const [compatibleSecretInput, setCompatibleSecretInput] = useState('')
  const [hints, setHints] = useState<Record<string, string | undefined>>({})

  const [fullTier, setFullTier] = useState(false)
  const [ingestBusy, setIngestBusy] = useState(false)
  const [ingestRows, setIngestRows] = useState<FileIngestResult[]>([])
  const [ingestLogLines, setIngestLogLines] = useState<string[]>([])
  const [pasteTitle, setPasteTitle] = useState('')
  const [pasteBody, setPasteBody] = useState('')

  const [sessions, setSessions] = useState<SessionFile[]>([])
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null)
  const [composer, setComposer] = useState('')
  const [sendBusy, setSendBusy] = useState(false)
  const [streamTail, setStreamTail] = useState('')
  const [saveTitle, setSaveTitle] = useState('Chat insight')

  const refreshHints = useCallback(async () => {
    try {
      const [o, a, g, c] = await Promise.all([
        invoke<string | null>('api_secret_hint', { provider: 'openai' }),
        invoke<string | null>('api_secret_hint', { provider: 'anthropic' }),
        invoke<string | null>('api_secret_hint', { provider: 'gemini' }),
        invoke<string | null>('api_secret_hint', { provider: 'compatible' }),
      ])
      setHints({
        openai: o ?? undefined,
        anthropic: a ?? undefined,
        gemini: g ?? undefined,
        compatible: c ?? undefined,
      })
    } catch {
      /* ignore */
    }
  }, [])

  const refreshSchema = useCallback(async (c: AppConfig) => {
    const sd = c.schemaDir
    if (!sd) {
      setSchemaStatus(null)
      return
    }
    try {
      const st = await invoke<SchemaStatus>('read_schema_status', { schemaDir: sd })
      setSchemaStatus(st)
    } catch {
      setSchemaStatus(null)
    }
  }, [])

  useEffect(() => {
    invoke<string>('get_platform_os').then(setPlatformOs).catch(() => setPlatformOs(''))
    loadCfg()
      .then((c) => {
        setCfg(c)
        refreshSchema(c)
        refreshHints()
      })
      .catch(() => setBanner({ kind: 'error', text: 'Could not load configuration.' }))
  }, [refreshHints, refreshSchema])

  const patchCfg = (partial: Partial<AppConfig>) => {
    setCfg((prev) => (prev ? { ...prev, ...partial } : prev))
  }

  const persistCfg = async (next: AppConfig) => {
    await saveCfg(next)
    setCfg(next)
    setBanner({ kind: 'success', text: 'Settings saved.' })
    refreshSchema(next)
  }

  const chooseVaultFolder = async () => {
    try {
      const dir = await invoke<string | null>('pick_vault_folder')
      if (!dir?.trim()) {
        return
      }
      patchCfg({ vaultRoot: dir })
      setBanner({ kind: 'success', text: `Vault folder: ${dir}` })
    } catch (e) {
      setBanner({ kind: 'error', text: `Folder picker failed: ${String(e)}` })
    }
  }

  const runSetup = async () => {
    if (!cfg?.vaultRoot?.trim()) {
      setBanner({ kind: 'error', text: 'Choose a vault folder first.' })
      return
    }
    try {
      const next = await invoke<AppConfig>('setup_vault_paths', {
        cfg,
        vaultRoot: cfg.vaultRoot.trim(),
      })
      setCfg(next)
      await refreshSchema(next)
      setBanner({ kind: 'success', text: 'Created raw/ and wiki/ (if missing). Paths saved.' })
    } catch (e) {
      setBanner({ kind: 'error', text: String(e) })
    }
  }

  const copyTemplates = async () => {
    if (!cfg?.schemaDir) {
      setBanner({ kind: 'error', text: 'Configure schema dir first (Setup assigns vault root).' })
      return
    }
    try {
      await invoke('copy_schema_templates', { schemaDir: cfg.schemaDir })
      await refreshSchema(cfg)
      setBanner({ kind: 'success', text: 'Bundled CLAUDE.md / llm-wiki.md copied when missing.' })
    } catch (e) {
      setBanner({ kind: 'error', text: String(e) })
    }
  }

  const saveSecret = async (provider: 'openai' | 'anthropic' | 'gemini' | 'compatible', secret: string) => {
    if (!secret.trim()) return
    try {
      await invoke('save_api_secret', { provider, secret: secret.trim() })
      setBanner({ kind: 'success', text: `${provider} key saved to OS keychain.` })
      if (provider === 'openai') setOpenaiSecretInput('')
      if (provider === 'anthropic') setAnthropicSecretInput('')
      if (provider === 'gemini') setGeminiSecretInput('')
      if (provider === 'compatible') setCompatibleSecretInput('')
      refreshHints()
    } catch (e) {
      setBanner({ kind: 'error', text: String(e) })
    }
  }

  const refreshModels = async () => {
    if (!cfg?.ollamaBaseUrl) return
    try {
      const models = await invoke<string[]>('fetch_ollama_models', {
        baseUrl: cfg.ollamaBaseUrl,
      })
      if (models.length && !cfg.ollamaModel) {
        patchCfg({ ollamaModel: models[0] })
      }
      setBanner({
        kind: 'success',
        text: models.length ? `Ollama tags: ${models.slice(0, 8).join(', ')}${models.length > 8 ? '…' : ''}` : 'No models from Ollama (is it running?).',
      })
    } catch {
      setBanner({ kind: 'error', text: 'Could not reach Ollama /api/tags.' })
    }
  }

  const subscribeIngestProgress = async () => {
    return listen<IngestProgressPayload>('ingest-progress', (ev) => {
      const line = formatIngestProgressLine(ev.payload)
      setIngestLogLines((prev) => [...prev, line])
    })
  }

  const runIngest = async () => {
    if (!cfg) return
    setIngestBusy(true)
    setIngestRows([])
    setIngestLogLines([])
    setBanner(null)
    const unlisten = await subscribeIngestProgress()
    try {
      const rows = await invoke<FileIngestResult[]>('run_ingest_cmd', {
        cfg,
        fullTier,
      })
      setIngestRows(rows)
      const errCount = rows.filter((r) => r.status === 'error').length
      if (errCount > 0) {
        setBanner({
          kind: 'error',
          text: `Ingest finished with ${errCount} error(s); see the table for details.`,
        })
      } else {
        setBanner({ kind: 'success', text: `Ingest finished (${rows.length} files scanned).` })
      }
    } catch (e) {
      setBanner({ kind: 'error', text: String(e) })
    } finally {
      unlisten()
      setIngestBusy(false)
    }
  }

  const pasteAndIngest = async () => {
    if (!cfg) return
    const body = pasteBody.trim()
    if (!body) {
      setBanner({ kind: 'error', text: 'Enter or paste some text to ingest.' })
      return
    }
    setIngestBusy(true)
    setIngestRows([])
    setIngestLogLines([])
    setBanner(null)
    const unlisten = await subscribeIngestProgress()
    try {
      const rows = await invoke<FileIngestResult[]>('ingest_pasted_text_cmd', {
        cfg,
        fullTier,
        payload: {
          content: pasteBody,
          fileStem: pasteTitle.trim() ? pasteTitle.trim() : undefined,
        },
      })
      setIngestRows(rows)
      const errCount = rows.filter((r) => r.status === 'error').length
      if (errCount > 0) {
        setBanner({
          kind: 'error',
          text: `Ingest finished with ${errCount} error(s); see the table for details.`,
        })
      } else {
        setPasteBody('')
        setBanner({
          kind: 'success',
          text: `Saved under raw/pastes/ and ingested (${rows.length} files scanned).`,
        })
      }
    } catch (e) {
      setBanner({ kind: 'error', text: String(e) })
    } finally {
      unlisten()
      setIngestBusy(false)
    }
  }

  const refreshSessions = useCallback(async () => {
    try {
      const list = await invoke<SessionFile[]>('list_chat_sessions')
      setSessions(list)
      if (!activeSessionId && list.length) {
        setActiveSessionId(list[0].id)
      }
    } catch {
      setSessions([])
    }
  }, [activeSessionId])

  useEffect(() => {
    if (tab === 'chat') refreshSessions()
  }, [tab, refreshSessions])

  const newSession = async () => {
    try {
      const s = await invoke<SessionFile>('new_chat_session')
      setSessions((prev) => [s, ...prev])
      setActiveSessionId(s.id)
      setStreamTail('')
    } catch (e) {
      setBanner({ kind: 'error', text: String(e) })
    }
  }

  const activeSession = sessions.find((s) => s.id === activeSessionId) ?? null

  const sendChat = async () => {
    if (!cfg || !activeSessionId || !composer.trim()) return
    setSendBusy(true)
    setStreamTail('')
    const userMessage = composer.trim()
    setComposer('')
    const un = await listen<string>('chat-token', (ev) => {
      const p = ev.payload
      if (p.includes('__ERROR__')) {
        setBanner({ kind: 'error', text: p.replace('__ERROR__', '').trim() })
      }
      setStreamTail((t) => t + p)
    })
    try {
      await invoke('chat_stream_cmd', {
        cfg,
        payload: {
          sessionId: activeSessionId,
          userMessage,
        },
      })
      await refreshSessions()
    } catch (e) {
      setBanner({ kind: 'error', text: String(e) })
    } finally {
      un()
      setSendBusy(false)
      setStreamTail('')
    }
  }

  const saveLastToWiki = async () => {
    if (!cfg || !activeSession) return
    const last = [...activeSession.messages].reverse().find((m) => m.role === 'assistant')
    if (!last?.content) {
      setBanner({ kind: 'error', text: 'No assistant message to save.' })
      return
    }
    try {
      const path = await invoke<string>('save_answer_to_wiki', {
        cfg,
        args: { title: saveTitle.trim() || 'Chat insight', bodyMarkdown: last.content },
      })
      setBanner({ kind: 'success', text: `Saved wiki/${path}` })
    } catch (e) {
      setBanner({ kind: 'error', text: String(e) })
    }
  }

  const rollMemory = async () => {
    if (!cfg || !activeSessionId) return
    try {
      await invoke('update_memory_roll_up', {
        cfg,
        sessionId: activeSessionId,
      })
      setBanner({ kind: 'success', text: 'Rolling memory updated (see app data context/memory.md).' })
    } catch (e) {
      setBanner({ kind: 'error', text: String(e) })
    }
  }

  const osHelp =
    cfg?.osHint === 'windows'
      ? 'Paths look like C:\\Users\\you\\Vault. Use Explorer to browse.'
      : cfg?.osHint === 'linux'
        ? 'Tip: xdg-open works for revealing folders. Ensure Secret Service is running for keyring.'
        : 'Paths look like /Users/you/Vault. Use Finder to browse.'

  if (!cfg) {
    return (
      <div className="app-shell">
        <p className="hint">Loading…</p>
      </div>
    )
  }

  return (
    <div className="app-shell">
      <header className="top-bar">
        <h1>Second Brain Lite</h1>
        <nav className="tabs" aria-label="Primary">
          <button type="button" className={tab === 'configuration' ? 'active' : ''} onClick={() => setTab('configuration')}>
            Configuration
          </button>
          <button type="button" className={tab === 'ingest' ? 'active' : ''} onClick={() => setTab('ingest')}>
            Ingest
          </button>
          <button type="button" className={tab === 'chat' ? 'active' : ''} onClick={() => setTab('chat')}>
            Chat
          </button>
        </nav>
      </header>

      {banner ? (
        <div className={`banner ${banner.kind}`} role="status">
          <div className="banner-row">
            <span className="banner-text">{banner.text}</span>
            <div className="banner-actions">
              {banner.kind === 'error' ? <CopyIconButton text={banner.text} label="Copy error" /> : null}
              <button type="button" className="btn secondary" style={{ padding: '0.2rem 0.5rem' }} onClick={() => setBanner(null)}>
                Dismiss
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {tab === 'configuration' ? (
        <section className="tab-panel">
          <p className="hint" style={{ marginTop: 0 }}>
            Machine OS (detected): <strong>{platformOs || 'unknown'}</strong>. {osHelp}
          </p>

          <fieldset className="cluster">
            <legend>Operating system hint</legend>
            <div className="os-radios">
              {(['auto', 'macos', 'windows', 'linux'] as const).map((v) => (
                <label key={v}>
                  <input type="radio" name="os" checked={cfg.osHint === v} onChange={() => patchCfg({ osHint: v })} />
                  {v === 'auto' ? 'Match this computer' : v}
                </label>
              ))}
            </div>
          </fieldset>

          <div className="field-grid">
            <label className="field">
              <span className="label-text">Vault root folder</span>
              <div className="row">
                <input type="text" readOnly value={cfg.vaultRoot ?? ''} placeholder="Choose folder…" />
                <button type="button" className="btn secondary" onClick={() => chooseVaultFolder()}>
                  Choose folder…
                </button>
                <button type="button" className="btn" onClick={() => runSetup()}>
                  Setup
                </button>
              </div>
              <span className="hint">Creates raw/ and wiki/ under this folder; schema defaults to vault root (CLAUDE.md + llm-wiki.md).</span>
            </label>

            <label className="field">
              <span className="label-text">Advanced — schema directory override</span>
              <input
                type="text"
                value={cfg.schemaDir ?? ''}
                onChange={(e) => patchCfg({ schemaDir: e.target.value })}
                placeholder="Leave empty to use vault root"
              />
            </label>

            <label className="field">
              <span className="label-text">Raw directory override</span>
              <input type="text" value={cfg.rawDir ?? ''} onChange={(e) => patchCfg({ rawDir: e.target.value })} placeholder="Default: &lt;vault&gt;/raw" />
            </label>

            <label className="field">
              <span className="label-text">Wiki directory override</span>
              <input type="text" value={cfg.wikiDir ?? ''} onChange={(e) => patchCfg({ wikiDir: e.target.value })} placeholder="Default: &lt;vault&gt;/wiki" />
            </label>

            <div className="row">
              <button type="button" className="btn secondary" onClick={() => copyTemplates()}>
                Copy template schemas
              </button>
              <span className="hint">
                Schema files:{' '}
                {schemaStatus
                  ? `${schemaStatus.claudeMd ? 'CLAUDE.md ✓' : 'CLAUDE.md ✗'} · ${schemaStatus.llmWikiMd ? 'llm-wiki.md ✓' : 'llm-wiki.md ✗'}`
                  : 'Setup paths first'}
              </span>
            </div>

            <fieldset className="cluster">
              <legend>Default model / provider</legend>
              <label className="field">
                <span className="label-text">Provider</span>
                <select value={cfg.defaultProvider} onChange={(e) => patchCfg({ defaultProvider: e.target.value })}>
                  <option value="ollama">Ollama (local)</option>
                  <option value="openai">OpenAI</option>
                  <option value="anthropic">Anthropic</option>
                  <option value="gemini">Google Gemini</option>
                  <option value="compatible">OpenAI-compatible API</option>
                </select>
              </label>

              <label className="field">
                <span className="label-text">
                  <input type="checkbox" checked={cfg.ollamaEnabled} onChange={(e) => patchCfg({ ollamaEnabled: e.target.checked })} /> Ollama enabled
                </span>
              </label>
              <label className="field">
                <span className="label-text">Ollama base URL</span>
                <div className="row">
                  <input type="text" value={cfg.ollamaBaseUrl} onChange={(e) => patchCfg({ ollamaBaseUrl: e.target.value })} />
                  <button type="button" className="btn secondary" onClick={() => refreshModels()}>
                    List models
                  </button>
                </div>
              </label>
              <label className="field">
                <span className="label-text">Ollama model id</span>
                <input type="text" value={cfg.ollamaModel} onChange={(e) => patchCfg({ ollamaModel: e.target.value })} placeholder="e.g. llama3.2" />
              </label>

              <label className="field">
                <span className="label-text">OpenAI model</span>
                <input type="text" value={cfg.openaiModel} onChange={(e) => patchCfg({ openaiModel: e.target.value })} />
                <span className="hint">
                  Stable ids such as gpt-5.4-mini or gpt-4o; legacy names like gpt-4o-latest / *-latest pointers are normalized automatically.
                </span>
              </label>
              <label className="field">
                <span className="label-text">OpenAI API key</span>
                <div className="row">
                  <input type="password" autoComplete="off" value={openaiSecretInput} onChange={(e) => setOpenaiSecretInput(e.target.value)} placeholder={hints.openai ?? 'Not saved'} />
                  <button type="button" className="btn secondary" onClick={() => saveSecret('openai', openaiSecretInput)}>
                    Save key
                  </button>
                </div>
                {hints.openai ? <span className="hint">Stored: {hints.openai}</span> : null}
              </label>

              <label className="field">
                <span className="label-text">Anthropic model</span>
                <input type="text" value={cfg.anthropicModel} onChange={(e) => patchCfg({ anthropicModel: e.target.value })} />
                <span className="hint">Use API ids such as claude-sonnet-4-6 or claude-haiku-4-5 (legacy names like claude-3-5-haiku-latest are mapped automatically).</span>
              </label>
              <label className="field">
                <span className="label-text">Anthropic API key</span>
                <div className="row">
                  <input type="password" autoComplete="off" value={anthropicSecretInput} onChange={(e) => setAnthropicSecretInput(e.target.value)} placeholder={hints.anthropic ?? 'Not saved'} />
                  <button type="button" className="btn secondary" onClick={() => saveSecret('anthropic', anthropicSecretInput)}>
                    Save key
                  </button>
                </div>
                {hints.anthropic ? <span className="hint">Stored: {hints.anthropic}</span> : null}
              </label>

              <label className="field">
                <span className="label-text">Gemini API base URL</span>
                <input
                  type="text"
                  value={cfg.geminiBaseUrl ?? ''}
                  onChange={(e) => patchCfg({ geminiBaseUrl: e.target.value })}
                  placeholder="https://generativelanguage.googleapis.com/v1beta"
                />
              </label>
              <label className="field">
                <span className="label-text">Gemini model id</span>
                <input type="text" value={cfg.geminiModel ?? ''} onChange={(e) => patchCfg({ geminiModel: e.target.value })} />
                <span className="hint">
                  Examples: gemini-3.1-flash-lite, gemini-3.1-pro-preview. Older ids like gemini-2.0-flash-latest map automatically.
                </span>
              </label>
              <label className="field">
                <span className="label-text">Gemini API key</span>
                <div className="row">
                  <input
                    type="password"
                    autoComplete="off"
                    value={geminiSecretInput}
                    onChange={(e) => setGeminiSecretInput(e.target.value)}
                    placeholder={hints.gemini ?? 'Not saved'}
                  />
                  <button type="button" className="btn secondary" onClick={() => saveSecret('gemini', geminiSecretInput)}>
                    Save key
                  </button>
                </div>
                {hints.gemini ? <span className="hint">Stored: {hints.gemini}</span> : null}
              </label>

              <label className="field">
                <span className="label-text">Compatible — base URL</span>
                <input type="text" value={cfg.compatibleBaseUrl} onChange={(e) => patchCfg({ compatibleBaseUrl: e.target.value })} placeholder="https://…/v1" />
              </label>
              <label className="field">
                <span className="label-text">Compatible — model id</span>
                <input type="text" value={cfg.compatibleModel} onChange={(e) => patchCfg({ compatibleModel: e.target.value })} />
                <span className="hint">OpenAI-style Chat Completions model names; *-latest aliases are normalized like OpenAI above.</span>
              </label>
              <label className="field">
                <span className="label-text">Compatible API key</span>
                <div className="row">
                  <input type="password" autoComplete="off" value={compatibleSecretInput} onChange={(e) => setCompatibleSecretInput(e.target.value)} placeholder={hints.compatible ?? 'Not saved'} />
                  <button type="button" className="btn secondary" onClick={() => saveSecret('compatible', compatibleSecretInput)}>
                    Save key
                  </button>
                </div>
                {hints.compatible ? <span className="hint">Stored: {hints.compatible}</span> : null}
              </label>
            </fieldset>

            <div className="row">
              <button type="button" className="btn" onClick={() => persistCfg(cfg)}>
                Save configuration
              </button>
            </div>
          </div>
        </section>
      ) : null}

      {tab === 'ingest' ? (
        <section className="tab-panel">
          <p>
            Scans <strong>raw/</strong> for{' '}
            <code>.md</code>, <code>.txt</code>, <code>.pdf</code>, <code>.docx</code>, <code>.html</code> / <code>.htm</code>, hashes them, and runs lite ingest into <strong>wiki/sources/</strong> using{' '}
            <strong>CLAUDE.md</strong> + <strong>llm-wiki.md</strong>. Each successful ingest appends a dated entry to <strong>wiki/log.md</strong> in your vault (open it in Obsidian or any editor).
          </p>
          <label className="field">
            <span className="label-text">
              <input type="checkbox" checked={fullTier} onChange={(e) => setFullTier(e.target.checked)} /> Full tier (richer glossary prompts)
            </span>
          </label>
          <div className="row">
            <button type="button" className="btn" disabled={ingestBusy} onClick={() => runIngest()}>
              {ingestBusy ? 'Ingesting…' : 'Run ingest'}
            </button>
            <span className="hint">Provider: {cfg.defaultProvider}</span>
          </div>

          <div className="ingest-paste-section">
            <h3 className="ingest-paste-heading">Paste text</h3>
            <p className="hint">
              Saves as <code>raw/pastes/&lt;name&gt;.md</code>, then runs the same ingest as above (model summarizes into{' '}
              <code>wiki/sources/</code>).
            </p>
            <label className="field">
              <span className="label-text">Filename (optional)</span>
              <input
                type="text"
                value={pasteTitle}
                onChange={(e) => setPasteTitle(e.target.value)}
                placeholder="e.g. meeting-notes — becomes pastes/meeting-notes.md"
                disabled={ingestBusy}
                autoComplete="off"
              />
            </label>
            <label className="field">
              <span className="label-text">Content</span>
              <textarea
                className="ingest-paste-textarea"
                value={pasteBody}
                onChange={(e) => setPasteBody(e.target.value)}
                placeholder="Paste notes, article text, transcript…"
                disabled={ingestBusy}
                rows={12}
              />
            </label>
            <div className="row">
              <button type="button" className="btn secondary" disabled={ingestBusy} onClick={() => pasteAndIngest()}>
                {ingestBusy ? 'Working…' : 'Save to raw & ingest'}
              </button>
            </div>
          </div>

          {ingestBusy || ingestLogLines.length ? (
            <div className="ingest-progress">
              <div className="ingest-progress-title">
                <span>Progress</span>
                {ingestLogLines.length ? (
                  <CopyIconButton text={ingestLogLines.join('\n')} label="Copy progress log" />
                ) : null}
              </div>
              <pre className="ingest-progress-pre">
                {ingestLogLines.length ? ingestLogLines.join('\n') : 'Starting…'}
              </pre>
            </div>
          ) : null}

          {ingestRows.length ? (
            <table className="ingest-table">
              <thead>
                <tr>
                  <th>Raw file</th>
                  <th>Status</th>
                  <th>Detail</th>
                  <th className="ingest-copy-cell" aria-label="Copy error">
                    Copy
                  </th>
                </tr>
              </thead>
              <tbody>
                {ingestRows.map((r) => (
                  <tr key={r.relativeRawPath}>
                    <td><code>{r.relativeRawPath}</code></td>
                    <td className={r.status === 'ok' ? 'status-ok' : r.status === 'skipped' ? 'status-skip' : 'status-err'}>{r.status}</td>
                    <td>{r.detail ?? ''}</td>
                    <td className="ingest-copy-cell">
                      {r.status === 'error' ? (
                        <CopyIconButton
                          text={[r.relativeRawPath, r.status, r.detail ?? ''].filter(Boolean).join('\n')}
                          label="Copy ingest error"
                        />
                      ) : null}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <p className="hint">Results appear here after ingest.</p>
          )}
        </section>
      ) : null}

      {tab === 'chat' ? (
        <section className="tab-panel">
          <p className="hint">
            Context pack: schema docs + index/glossary excerpts + BM25 retrieval over wiki + rolling memory. Streaming via events. Current provider:{' '}
            <strong>{cfg.defaultProvider}</strong>
          </p>
          <div className="chat-layout">
            <div className="session-list">
              <div className="row" style={{ marginBottom: '0.5rem' }}>
                <button type="button" className="btn secondary" onClick={() => newSession()}>
                  New chat
                </button>
              </div>
              {sessions.map((s) => (
                <button key={s.id} type="button" className={s.id === activeSessionId ? 'active' : ''} onClick={() => setActiveSessionId(s.id)}>
                  {s.title}
                </button>
              ))}
              {!sessions.length ? <p className="hint">No sessions yet.</p> : null}
            </div>
            <div>
              <div className="messages">
                {activeSession?.messages.map((m, i) => (
                  <div key={`${m.ts ?? ''}-${i}`} className={`msg ${m.role}`}>
                    <div className="msg-head">
                      <div className="role">{m.role}</div>
                      <CopyIconButton text={m.content} label={`Copy ${m.role} message`} />
                    </div>
                    <div className="body">
                      {m.role === 'assistant' ? <ReactMarkdown>{m.content}</ReactMarkdown> : m.content}
                    </div>
                  </div>
                ))}
                {streamTail ? (
                  <div className="msg assistant">
                    <div className="msg-head">
                      <div className="role">assistant (streaming)</div>
                      <CopyIconButton text={chatClipboardText(streamTail)} label="Copy assistant reply" />
                    </div>
                    <div className="body">
                      <ReactMarkdown>{streamTail}</ReactMarkdown>
                    </div>
                  </div>
                ) : null}
              </div>
              <label className="field">
                <span className="label-text">Message</span>
                <textarea value={composer} onChange={(e) => setComposer(e.target.value)} placeholder="Ask something grounded in your wiki…" />
              </label>
              <div className="row">
                <button type="button" className="btn" disabled={sendBusy || !activeSessionId} onClick={() => sendChat()}>
                  Send
                </button>
                <label className="field" style={{ flex: 1, marginBottom: 0 }}>
                  <span className="label-text">Title when saving answer</span>
                  <input type="text" value={saveTitle} onChange={(e) => setSaveTitle(e.target.value)} />
                </label>
                <button type="button" className="btn secondary" onClick={() => saveLastToWiki()}>
                  Save answer to wiki
                </button>
                <button type="button" className="btn secondary" onClick={() => rollMemory()}>
                  Update rolling memory
                </button>
              </div>
            </div>
          </div>
        </section>
      ) : null}
    </div>
  )
}
