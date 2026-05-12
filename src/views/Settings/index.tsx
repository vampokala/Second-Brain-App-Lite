import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { useConfig } from '@/hooks/useConfig'
import { DEFAULT_CHAT_PERSONA, DEFAULT_STUDENT_GRADE, FALLBACK_GRADE_OPTIONS, FALLBACK_PERSONA_OPTIONS, normalizeChatPersona } from '@/lib/personas'
import { invoke } from '@tauri-apps/api/core'
import { CheckCircle2, FolderOpen, XCircle } from 'lucide-react'
import { useEffect, useState } from 'react'

type Banner = { kind: 'success' | 'error'; text: string } | null

function FieldRow({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <p className="text-xs text-[var(--color-muted-foreground)]">{hint}</p>}
    </div>
  )
}

function KeyRow({
  label,
  hint,
  value,
  onChange,
  placeholder,
  storedHint,
  onSave,
}: {
  label: string
  hint?: string
  value: string
  onChange: (v: string) => void
  placeholder: string
  storedHint?: string
  onSave: () => void
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label>{label}</Label>
      <div className="flex gap-2">
        <Input
          type="password"
          autoComplete="off"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={storedHint ? `Stored: ${storedHint}` : placeholder}
          className="flex-1"
        />
        <Button variant="secondary" size="sm" onClick={onSave} disabled={!value.trim()}>
          Save key
        </Button>
      </div>
      {storedHint && (
        <p className="text-xs text-[var(--color-success)] flex items-center gap-1">
          <CheckCircle2 size={11} /> Stored: {storedHint}
        </p>
      )}
      {hint && <p className="text-xs text-[var(--color-muted-foreground)]">{hint}</p>}
    </div>
  )
}

export default function SettingsView({ onBanner }: { onBanner: (b: Banner) => void }) {
  const { cfg, patchCfg, setCfg, persistCfg, schemaStatus, platformOs, hints, refreshHints, refreshSchema } = useConfig()

  const [openaiKey, setOpenaiKey] = useState('')
  const [anthropicKey, setAnthropicKey] = useState('')
  const [geminiKey, setGeminiKey] = useState('')
  const [compatibleKey, setCompatibleKey] = useState('')
  const [braveKey, setBraveKey] = useState('')

  const [openaiListed, setOpenaiListed] = useState<string[]>([])
  const [anthropicListed, setAnthropicListed] = useState<string[]>([])
  const [geminiListed, setGeminiListed] = useState<string[]>([])
  const [listingRemote, setListingRemote] = useState<null | 'openai' | 'anthropic' | 'gemini'>(null)

  const [personaOptions, setPersonaOptions] = useState<{ id: string; label: string }[]>([])
  const [gradeOptions, setGradeOptions] = useState<{ id: string; label: string }[]>([])

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const [p, g] = await Promise.all([
          invoke<{ id: string; label: string }[]>('list_chat_personas'),
          invoke<{ id: string; label: string }[]>('list_student_grade_options'),
        ])
        if (!cancelled) {
          setPersonaOptions(p)
          setGradeOptions(g)
        }
      } catch {
        /* ignore */
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  if (!cfg) {
    return (
      <div className="flex min-h-full w-full items-center justify-center px-4 text-[var(--color-muted-foreground)] text-sm lg:px-6">
        Loading…
      </div>
    )
  }

  const osHelp =
    cfg.osHint === 'windows'
      ? 'Paths look like C:\\Users\\you\\Vault.'
      : cfg.osHint === 'linux'
        ? 'Ensure Secret Service is running for keyring.'
        : 'Paths look like /Users/you/Vault.'

  const chooseVaultFolder = async () => {
    try {
      const dir = await invoke<string | null>('pick_vault_folder')
      if (!dir?.trim()) return
      patchCfg({ vaultRoot: dir })
      onBanner({ kind: 'success', text: `Vault folder set: ${dir}` })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const runSetup = async () => {
    if (!cfg.vaultRoot?.trim()) { onBanner({ kind: 'error', text: 'Choose a vault folder first.' }); return }
    try {
      const next = await invoke<typeof cfg>('setup_vault_paths', { cfg, vaultRoot: cfg.vaultRoot.trim() })
      await refreshSchema(next)
      onBanner({ kind: 'success', text: 'Created raw/ and wiki/ (if missing). Paths saved.' })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const copyTemplates = async () => {
    if (!cfg.schemaDir) { onBanner({ kind: 'error', text: 'Configure schema dir first.' }); return }
    try {
      await invoke('copy_schema_templates', { schemaDir: cfg.schemaDir })
      await refreshSchema(cfg)
      onBanner({ kind: 'success', text: 'Template schemas copied.' })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const saveKey = async (provider: 'openai' | 'anthropic' | 'gemini' | 'compatible' | 'brave', secret: string, clear: () => void) => {
    if (!secret.trim()) return
    try {
      await invoke('save_api_secret', { provider, secret: secret.trim() })
      clear()
      await refreshHints()
      onBanner({ kind: 'success', text: `${provider} key saved securely.` })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const refreshModels = async () => {
    if (!cfg.ollamaBaseUrl) return
    try {
      const models = await invoke<string[]>('fetch_ollama_models', { baseUrl: cfg.ollamaBaseUrl })
      if (models.length && !cfg.ollamaModel) patchCfg({ ollamaModel: models[0] })
      onBanner({ kind: 'success', text: models.length ? `Ollama models: ${models.slice(0, 6).join(', ')}` : 'No models from Ollama.' })
    } catch {
      onBanner({ kind: 'error', text: 'Could not reach Ollama /api/tags.' })
    }
  }

  const listRemoteModels = async (which: 'openai' | 'anthropic' | 'gemini') => {
    setListingRemote(which)
    try {
      if (which === 'openai') {
        const models = await invoke<string[]>('fetch_openai_models')
        setOpenaiListed(models)
        setCfg((prev) => {
          if (!prev || !models.length) return prev
          if (models.includes(prev.openaiModel)) return prev
          return { ...prev, openaiModel: models[0] }
        })
        onBanner({
          kind: 'success',
          text: models.length ? `OpenAI: loaded ${models.length} chat-capable models (filtered).` : 'OpenAI returned no matching models.',
        })
      } else if (which === 'anthropic') {
        const models = await invoke<string[]>('fetch_anthropic_models')
        setAnthropicListed(models)
        setCfg((prev) => {
          if (!prev || !models.length) return prev
          if (models.includes(prev.anthropicModel)) return prev
          return { ...prev, anthropicModel: models[0] }
        })
        onBanner({
          kind: 'success',
          text: models.length ? `Anthropic: loaded ${models.length} models.` : 'Anthropic returned no models.',
        })
      } else {
        const models = await invoke<string[]>('fetch_gemini_models', { geminiBaseUrl: cfg.geminiBaseUrl ?? '' })
        setGeminiListed(models)
        setCfg((prev) => {
          if (!prev || !models.length) return prev
          const cur = prev.geminiModel ?? ''
          if (models.includes(cur)) return prev
          return { ...prev, geminiModel: models[0] }
        })
        onBanner({
          kind: 'success',
          text: models.length ? `Gemini: loaded ${models.length} generateContent models.` : 'Gemini returned no models.',
        })
      }
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    } finally {
      setListingRemote(null)
    }
  }

  const handleSave = async () => {
    try {
      await persistCfg(cfg)
      onBanner({ kind: 'success', text: 'Settings saved.' })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const handleSaveChat = async () => {
    try {
      await persistCfg(cfg)
      onBanner({ kind: 'success', text: 'Chat settings saved.' })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  return (
    <div className="flex min-h-full w-full flex-col gap-5 px-4 py-6 box-border lg:px-6">
      <div>
        <h1 className="text-lg font-semibold text-[var(--color-foreground)]">Settings</h1>
        <p className="text-sm text-[var(--color-muted-foreground)] mt-0.5">
          OS detected: <strong className="text-[var(--color-foreground)]">{platformOs || 'unknown'}</strong> — {osHelp}
        </p>
      </div>

      {/* Vault */}
      <Card>
        <CardHeader>
          <CardTitle>Vault</CardTitle>
          <CardDescription>Choose where your raw/ and wiki/ folders live.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <FieldRow label="Vault root folder" hint="Creates raw/ and wiki/ under this folder.">
            <div className="flex gap-2">
              <Input readOnly value={cfg.vaultRoot ?? ''} placeholder="Choose folder…" className="flex-1" />
              <Button variant="secondary" size="sm" onClick={chooseVaultFolder}>
                <FolderOpen size={14} className="mr-1.5" /> Choose
              </Button>
              <Button size="sm" onClick={runSetup}>Setup</Button>
            </div>
          </FieldRow>
          <div className="flex items-center gap-3 flex-wrap">
            <Button variant="secondary" size="sm" onClick={copyTemplates}>Copy template schemas</Button>
            {schemaStatus ? (
              <div className="flex items-center gap-2 text-xs text-[var(--color-muted-foreground)]">
                <span className={schemaStatus.claudeMd ? 'text-[var(--color-success)]' : 'text-[var(--color-destructive)]'}>
                  {schemaStatus.claudeMd ? <CheckCircle2 size={13} className="inline mr-0.5" /> : <XCircle size={13} className="inline mr-0.5" />}
                  CLAUDE.md
                </span>
                <span className={schemaStatus.llmWikiMd ? 'text-[var(--color-success)]' : 'text-[var(--color-destructive)]'}>
                  {schemaStatus.llmWikiMd ? <CheckCircle2 size={13} className="inline mr-0.5" /> : <XCircle size={13} className="inline mr-0.5" />}
                  llm-wiki.md
                </span>
              </div>
            ) : <span className="text-xs text-[var(--color-muted-foreground)]">Setup paths first</span>}
          </div>
          <details className="group">
            <summary className="text-xs text-[var(--color-muted-foreground)] cursor-pointer select-none hover:text-[var(--color-foreground)] transition-colors">
              Advanced path overrides
            </summary>
            <div className="flex flex-col gap-3 mt-3">
              <FieldRow label="Schema directory override">
                <Input value={cfg.schemaDir ?? ''} onChange={(e) => patchCfg({ schemaDir: e.target.value })} placeholder="Leave empty to use vault root" />
              </FieldRow>
              <FieldRow label="Raw directory override">
                <Input value={cfg.rawDir ?? ''} onChange={(e) => patchCfg({ rawDir: e.target.value })} placeholder="Default: <vault>/raw" />
              </FieldRow>
              <FieldRow label="Wiki directory override">
                <Input value={cfg.wikiDir ?? ''} onChange={(e) => patchCfg({ wikiDir: e.target.value })} placeholder="Default: <vault>/wiki" />
              </FieldRow>
            </div>
          </details>
        </CardContent>
        <CardFooter>
          <Button size="sm" onClick={handleSave}>Save vault settings</Button>
        </CardFooter>
      </Card>

      {/* Provider & Models */}
      <Card>
        <CardHeader>
          <CardTitle>Provider &amp; Models</CardTitle>
          <CardDescription>Configure which AI provider and models to use for chat and ingest.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <FieldRow label="OS hint">
            <Select value={cfg.osHint} onChange={(e) => patchCfg({ osHint: e.target.value })}>
              <option value="auto">Match this computer</option>
              <option value="macos">macOS</option>
              <option value="windows">Windows</option>
              <option value="linux">Linux</option>
            </Select>
          </FieldRow>
          <FieldRow label="Default provider">
            <Select value={cfg.defaultProvider} onChange={(e) => patchCfg({ defaultProvider: e.target.value })}>
              <option value="ollama">Ollama (local)</option>
              <option value="openai">OpenAI</option>
              <option value="anthropic">Anthropic</option>
              <option value="gemini">Google Gemini</option>
              <option value="compatible">OpenAI-compatible API</option>
            </Select>
          </FieldRow>

          <div className="border-t border-[var(--color-border)] pt-4 flex flex-col gap-4">
            <div className="flex items-center gap-2">
              <input id="ollama-enabled" type="checkbox" checked={cfg.ollamaEnabled} onChange={(e) => patchCfg({ ollamaEnabled: e.target.checked })} className="rounded" />
              <Label htmlFor="ollama-enabled">Ollama enabled</Label>
            </div>
            <FieldRow label="Ollama base URL">
              <div className="flex gap-2">
                <Input value={cfg.ollamaBaseUrl} onChange={(e) => patchCfg({ ollamaBaseUrl: e.target.value })} className="flex-1" />
                <Button variant="secondary" size="sm" onClick={refreshModels}>List models</Button>
              </div>
            </FieldRow>
            <FieldRow label="Ollama model ID">
              <Input value={cfg.ollamaModel} onChange={(e) => patchCfg({ ollamaModel: e.target.value })} placeholder="e.g. llama3.2" />
            </FieldRow>
          </div>

          <div className="border-t border-[var(--color-border)] pt-4 flex flex-col gap-4">
            <FieldRow label="OpenAI model" hint="Save your OpenAI key below, then List models. You can still type a model id manually.">
              <div className="flex gap-2">
                <Input
                  className="flex-1"
                  value={cfg.openaiModel}
                  onChange={(e) => patchCfg({ openaiModel: e.target.value })}
                />
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={listingRemote === 'openai'}
                  onClick={() => void listRemoteModels('openai')}
                >
                  List models
                </Button>
              </div>
              {openaiListed.length > 0 ? (
                <Select
                  className="mt-2"
                  value={cfg.openaiModel}
                  onChange={(e) => patchCfg({ openaiModel: e.target.value })}
                >
                  {openaiListed.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </Select>
              ) : null}
            </FieldRow>
            <FieldRow label="Anthropic model" hint="Save your Anthropic key below, then List models.">
              <div className="flex gap-2">
                <Input
                  className="flex-1"
                  value={cfg.anthropicModel}
                  onChange={(e) => patchCfg({ anthropicModel: e.target.value })}
                />
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={listingRemote === 'anthropic'}
                  onClick={() => void listRemoteModels('anthropic')}
                >
                  List models
                </Button>
              </div>
              {anthropicListed.length > 0 ? (
                <Select
                  className="mt-2"
                  value={cfg.anthropicModel}
                  onChange={(e) => patchCfg({ anthropicModel: e.target.value })}
                >
                  {anthropicListed.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </Select>
              ) : null}
            </FieldRow>
            <FieldRow label="Gemini API base URL">
              <Input value={cfg.geminiBaseUrl ?? ''} onChange={(e) => patchCfg({ geminiBaseUrl: e.target.value })} placeholder="https://generativelanguage.googleapis.com/v1beta" />
            </FieldRow>
            <FieldRow label="Gemini model ID" hint="Save your Gemini key below, set base URL above, then List models.">
              <div className="flex gap-2">
                <Input
                  className="flex-1"
                  value={cfg.geminiModel ?? ''}
                  onChange={(e) => patchCfg({ geminiModel: e.target.value })}
                />
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={listingRemote === 'gemini'}
                  onClick={() => void listRemoteModels('gemini')}
                >
                  List models
                </Button>
              </div>
              {geminiListed.length > 0 ? (
                <Select
                  className="mt-2"
                  value={cfg.geminiModel ?? ''}
                  onChange={(e) => patchCfg({ geminiModel: e.target.value })}
                >
                  {geminiListed.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </Select>
              ) : null}
            </FieldRow>
            <FieldRow label="Compatible base URL">
              <Input value={cfg.compatibleBaseUrl} onChange={(e) => patchCfg({ compatibleBaseUrl: e.target.value })} placeholder="https://…/v1" />
            </FieldRow>
            <FieldRow label="Compatible model ID">
              <Input value={cfg.compatibleModel} onChange={(e) => patchCfg({ compatibleModel: e.target.value })} />
            </FieldRow>
          </div>
        </CardContent>
        <CardFooter>
          <Button size="sm" onClick={handleSave}>Save provider settings</Button>
        </CardFooter>
      </Card>

      {/* Chat personas */}
      <Card>
        <CardHeader>
          <CardTitle>Chat</CardTitle>
          <CardDescription>
            Persona shapes tone and priorities for every chat message. Wiki and web toggles still control evidence sources.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <FieldRow
            label="Persona"
            hint="Applies to all chats until you change it. The current persona appears next to the checkboxes on the Chat screen."
          >
            <Select
              value={normalizeChatPersona(cfg.chatPersona ?? DEFAULT_CHAT_PERSONA)}
              onChange={(e) => patchCfg({ chatPersona: e.target.value })}
            >
              {(personaOptions.length ? personaOptions : FALLBACK_PERSONA_OPTIONS).map((o) => (
                <option key={o.id} value={o.id}>{o.label}</option>
              ))}
            </Select>
          </FieldRow>
          {normalizeChatPersona(cfg.chatPersona ?? DEFAULT_CHAT_PERSONA) === 'student' && (
            <FieldRow label="Grade" hint="Kindergarten through Grade 12 (high school).">
              <Select
                value={cfg.studentGrade?.trim() || DEFAULT_STUDENT_GRADE}
                onChange={(e) => patchCfg({ studentGrade: e.target.value })}
              >
                {(gradeOptions.length ? gradeOptions : FALLBACK_GRADE_OPTIONS).map((o) => (
                  <option key={o.id} value={o.id}>{o.label}</option>
                ))}
              </Select>
            </FieldRow>
          )}
          <FieldRow
            label="Additional persona instructions"
            hint="Optional. Appended after the built-in persona text on every send—for example stack, team norms, or how deep to go."
          >
            <Textarea
              value={cfg.personaPromptAddon ?? ''}
              onChange={(e) => patchCfg({ personaPromptAddon: e.target.value })}
              placeholder="e.g. Prefer TypeScript examples; we use trunk-based development."
              className="min-h-24 resize-y"
            />
          </FieldRow>
        </CardContent>
        <CardFooter>
          <Button size="sm" onClick={() => void handleSaveChat()}>Save chat settings</Button>
        </CardFooter>
      </Card>

      {/* API Keys */}
      <Card>
        <CardHeader>
          <CardTitle>API Keys</CardTitle>
          <CardDescription>Keys are encrypted locally (app data) — never written to your vault.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <KeyRow label="OpenAI API key" value={openaiKey} onChange={setOpenaiKey} placeholder="sk-…" storedHint={hints.openai} onSave={() => saveKey('openai', openaiKey, () => setOpenaiKey(''))} />
          <KeyRow label="Anthropic API key" value={anthropicKey} onChange={setAnthropicKey} placeholder="sk-ant-…" storedHint={hints.anthropic} onSave={() => saveKey('anthropic', anthropicKey, () => setAnthropicKey(''))} />
          <KeyRow label="Gemini API key" value={geminiKey} onChange={setGeminiKey} placeholder="AIza…" storedHint={hints.gemini} onSave={() => saveKey('gemini', geminiKey, () => setGeminiKey(''))} />
          <KeyRow label="Compatible API key" value={compatibleKey} onChange={setCompatibleKey} placeholder="Bearer token or key" storedHint={hints.compatible} onSave={() => saveKey('compatible', compatibleKey, () => setCompatibleKey(''))} />
          <KeyRow
            label="Brave Search API key"
            hint="Used when Chat enables “Web search”. Create a key at Brave Search API (developer dashboard)."
            value={braveKey}
            onChange={setBraveKey}
            placeholder="BSA…"
            storedHint={hints.brave}
            onSave={() => saveKey('brave', braveKey, () => setBraveKey(''))}
          />
        </CardContent>
      </Card>
    </div>
  )
}
