import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { CopyButton } from '@/components/ui/copy-button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useIngest, formatIngestProgressLine } from '@/hooks/useIngest'
import { ingestLlmSummary } from '@/lib/llm-display'
import { cn } from '@/lib/utils'
import type {
  AppConfig,
  ChatExcerpt,
  CursorTranscriptFile,
  CursorWorkspaceEntry,
  FileIngestResult,
  IngestCommitPreview,
  IngestProgressPayload,
  IngestTrackMode,
  IngestUiHints,
  PrepareCursorAssistResponse,
  SessionFile,
  TrackInference,
} from '@/types'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { BrainCircuit, CheckCircle2, FileText, FolderOpen, Loader2, SkipForward, XCircle } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

type Banner = { kind: 'success' | 'error'; text: string } | null

function StatusIcon({ status }: { status: string }) {
  if (status === 'ok') return <CheckCircle2 size={14} className="text-[var(--color-success)] shrink-0" />
  if (status === 'skipped' || status === 'cancelled') return <SkipForward size={14} className="text-[var(--color-muted-foreground)] shrink-0" />
  return <XCircle size={14} className="text-[var(--color-destructive)] shrink-0" />
}

export default function IngestView({ cfg, onBanner }: { cfg: AppConfig | null; onBanner: (b: Banner) => void }) {
  const { busy, rows, setRows, logLines, setLogLines, runIngest, pasteAndIngest, ingestUrl, listTracks, inferTrack, cancelIngest } = useIngest()
  const [fullTier, setFullTier] = useState(false)
  const [pasteTitle, setPasteTitle] = useState('')
  const [pasteBody, setPasteBody] = useState('')
  const [urlInput, setUrlInput] = useState('')
  const [urlStem, setUrlStem] = useState('')
  const [showPaste, setShowPaste] = useState(false)
  const [memoryBusy, setMemoryBusy] = useState(false)
  const [tracks, setTracks] = useState<string[]>([])
  const [trackMode, setTrackMode] = useState<IngestTrackMode>('auto')
  const [existingTrack, setExistingTrack] = useState('')
  const [newTrack, setNewTrack] = useState('')
  const [autoInference, setAutoInference] = useState<TrackInference | null>(null)
  const [ingestHints, setIngestHints] = useState<IngestUiHints | null>(null)
  /** Shown on this tab when ingest fails or returns per-file errors (banner alone is easy to miss). */
  const [ingestError, setIngestError] = useState<string | null>(null)

  const [cursorAssistOpen, setCursorAssistOpen] = useState(false)
  const [cursorRawRel, setCursorRawRel] = useState<string | null>(null)
  const [cursorPromptPack, setCursorPromptPack] = useState('')
  const [cursorModelJson, setCursorModelJson] = useState('')
  const [cursorPreview, setCursorPreview] = useState<IngestCommitPreview | null>(null)
  const [cursorAssistBusy, setCursorAssistBusy] = useState(false)

  const [archiveOpen, setArchiveOpen] = useState(false)
  const [hashFilter, setHashFilter] = useState('')
  const [maxAgeDays, setMaxAgeDays] = useState('30')
  const [vaultFilter, setVaultFilter] = useState('')
  const [workspaces, setWorkspaces] = useState<CursorWorkspaceEntry[]>([])
  const [wsSelected, setWsSelected] = useState('')
  const [transcripts, setTranscripts] = useState<CursorTranscriptFile[]>([])
  const [txSelected, setTxSelected] = useState('')
  const [archiveExcerpt, setArchiveExcerpt] = useState<ChatExcerpt | null>(null)
  const [destEnrich, setDestEnrich] = useState(true)
  const [destWiki, setDestWiki] = useState(false)
  const [destSession, setDestSession] = useState(false)
  const [archiveBusy, setArchiveBusy] = useState(false)

  useEffect(() => {
    if (!cfg) return
    listTracks(cfg).then(setTracks).catch(() => setTracks([]))
  }, [cfg, listTracks])

  useEffect(() => {
    if (!cfg) return
    invoke<IngestUiHints>('get_ingest_ui_hints', { cfg })
      .then(setIngestHints)
      .catch((e) => {
        setIngestHints(null)
        setIngestError(`Could not load ingest hints: ${String(e)}`)
      })
  }, [cfg])

  const resolvedTrackId = useMemo(() => {
    if (trackMode === 'existing') return existingTrack.trim() || undefined
    if (trackMode === 'new') return newTrack.trim() || undefined
    return undefined
  }, [trackMode, existingTrack, newTrack])
  const invalidTrackSelection = (trackMode === 'existing' || trackMode === 'new') && !resolvedTrackId

  useEffect(() => {
    if (!cfg || trackMode !== 'auto') { setAutoInference(null); return }
    const source = (pasteBody.trim() || urlInput.trim())
    if (!source) { setAutoInference(null); return }
    const timer = setTimeout(() => {
      inferTrack(cfg, source, urlInput.trim() || undefined).then(setAutoInference).catch(() => setAutoInference(null))
    }, 250)
    return () => clearTimeout(timer)
  }, [cfg, trackMode, pasteBody, urlInput, inferTrack])

  const maybeWarnLowConfidence = (): boolean => {
    if (trackMode !== 'auto') return true
    if (!autoInference?.trackId) return true
    if (autoInference.confidence >= 0.45) return true
    return window.confirm(
      `Auto-detect confidence is low (${Math.round(autoInference.confidence * 100)}%) for track "${autoInference.trackId}". Continue anyway?`,
    )
  }

  const handleRollupToMemory = async () => {
    if (!pasteBody.trim()) { onBanner({ kind: 'error', text: 'Enter some text to roll up.' }); return }
    if (!cfg) return
    setMemoryBusy(true)
    try {
      await invoke('rollup_content_to_memory', { cfg, content: pasteBody })
      onBanner({ kind: 'success', text: 'Rolling memory updated.' })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    } finally {
      setMemoryBusy(false)
    }
  }

  if (!cfg) return <div className="flex items-center justify-center h-full text-[var(--color-muted-foreground)] text-sm">Loading config…</div>

  const llmLine = ingestLlmSummary(cfg)

  const handleRunIngest = async () => {
    setIngestError(null)
    try {
      const result = await runIngest(cfg, fullTier, resolvedTrackId)
      const errs = result.filter((r) => r.status === 'error')
      if (errs.length > 0) {
        const lines = errs.map((r) => `${r.relativeRawPath}: ${r.detail ?? r.status}`).join('\n')
        setIngestError(`${errs.length} error(s):\n${lines}`)
        onBanner({ kind: 'error', text: `Ingest finished with ${errs.length} error(s). See message on Ingest tab.` })
      } else {
        setIngestError(null)
        onBanner({ kind: 'success', text: `Ingest finished — ${result.length} files scanned.` })
      }
    } catch (e) {
      const msg = String(e)
      setIngestError(msg)
      onBanner({ kind: 'error', text: msg })
    }
  }

  const handleStopIngest = async () => {
    const shouldStop = window.confirm('Stop ingest now? You can restart it anytime.')
    if (!shouldStop) return
    try {
      await cancelIngest()
      onBanner({ kind: 'success', text: 'Stopping ingest…' })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const handlePasteIngest = async () => {
    if (!pasteBody.trim()) { onBanner({ kind: 'error', text: 'Enter some text to ingest.' }); return }
    if (!maybeWarnLowConfidence()) return
    setIngestError(null)
    try {
      const result = await pasteAndIngest(cfg, fullTier, pasteBody, pasteTitle, resolvedTrackId, trackMode === 'auto')
      const errs = result.filter((r) => r.status === 'error')
      if (errs.length > 0) {
        const lines = errs.map((r) => `${r.relativeRawPath}: ${r.detail ?? r.status}`).join('\n')
        setIngestError(`${errs.length} error(s):\n${lines}`)
        onBanner({ kind: 'error', text: `Paste ingest: ${errs.length} error(s). See message on Ingest tab.` })
      } else {
        setIngestError(null)
        setPasteBody('')
        onBanner({ kind: 'success', text: `Saved to raw/<track>/pastes and ingested (${result.length} files).` })
        listTracks(cfg).then(setTracks).catch(() => undefined)
      }
    } catch (e) {
      const msg = String(e)
      setIngestError(msg)
      onBanner({ kind: 'error', text: msg })
    }
  }

  const handleUrlIngest = async () => {
    if (!urlInput.trim()) { onBanner({ kind: 'error', text: 'Enter a URL to ingest.' }); return }
    if (!maybeWarnLowConfidence()) return
    setIngestError(null)
    try {
      const result = await ingestUrl(cfg, fullTier, urlInput, urlStem, resolvedTrackId, trackMode === 'auto')
      const errs = result.filter((r) => r.status === 'error')
      if (errs.length > 0) {
        const lines = errs.map((r) => `${r.relativeRawPath}: ${r.detail ?? r.status}`).join('\n')
        setIngestError(`${errs.length} error(s):\n${lines}`)
        onBanner({ kind: 'error', text: `URL ingest: ${errs.length} error(s). See message on Ingest tab.` })
      } else {
        setIngestError(null)
        setUrlInput('')
        setUrlStem('')
        onBanner({ kind: 'success', text: `URL fetched and ingested (${result.length} files).` })
        listTracks(cfg).then(setTracks).catch(() => undefined)
      }
    } catch (e) {
      const msg = String(e)
      setIngestError(msg)
      onBanner({ kind: 'error', text: msg })
    }
  }

  const subscribeIngestProgress = async () =>
    listen<IngestProgressPayload>('ingest-progress', (ev) => {
      setLogLines((prev) => [...prev, formatIngestProgressLine(ev.payload)])
    })

  const handlePrepareCursorAssist = async () => {
    if (!cfg) return
    if (!pasteBody.trim()) {
      onBanner({ kind: 'error', text: 'Enter content in Paste first (same as Save to raw & ingest).' })
      setShowPaste(true)
      return
    }
    if (!maybeWarnLowConfidence()) return
    setIngestError(null)
    setCursorAssistBusy(true)
    setCursorPreview(null)
    const unlisten = await subscribeIngestProgress()
    try {
      const res = await invoke<PrepareCursorAssistResponse>('prepare_cursor_assisted_ingest', {
        cfg,
        fullTier,
        payload: {
          content: pasteBody,
          fileStem: pasteTitle.trim() || undefined,
          trackId: resolvedTrackId,
          autoDetectTrack: trackMode === 'auto',
        },
      })
      setCursorRawRel(res.rawRel)
      setCursorPromptPack(res.promptPack)
      onBanner({ kind: 'success', text: `Saved raw/${res.rawRel}. Copy the prompt pack to Cursor Chat.` })
    } catch (e) {
      const msg = String(e)
      setIngestError(msg)
      onBanner({ kind: 'error', text: msg })
    } finally {
      unlisten()
      setCursorAssistBusy(false)
    }
  }

  const handlePreviewCursorCommit = async () => {
    if (!cfg || !cursorRawRel || !cursorModelJson.trim()) return
    setIngestError(null)
    try {
      const p = await invoke<IngestCommitPreview>('preview_cursor_assisted_commit_cmd', {
        cfg,
        rawRel: cursorRawRel,
        pastedModelJson: cursorModelJson,
      })
      setCursorPreview(p)
      onBanner({ kind: 'success', text: 'JSON valid — review preview below, then commit.' })
    } catch (e) {
      const msg = String(e)
      setIngestError(msg)
      setCursorPreview(null)
      onBanner({ kind: 'error', text: msg })
    }
  }

  const handleCommitCursorAssist = async () => {
    if (!cfg || !cursorRawRel || !cursorModelJson.trim()) return
    if (!cursorPreview) {
      onBanner({ kind: 'error', text: 'Run “Validate JSON” first.' })
      return
    }
    if (
      !window.confirm(
        `Commit to wiki/${cursorPreview.wikiSourceRel}?\n\nTitle: ${cursorPreview.title}\nTrack: ${cursorPreview.trackId ?? 'inbox'}`,
      )
    ) {
      return
    }
    setIngestError(null)
    setCursorAssistBusy(true)
    const unlisten = await subscribeIngestProgress()
    try {
      const r = await invoke<FileIngestResult>('commit_cursor_assisted_ingest_cmd', {
        cfg,
        rawRel: cursorRawRel,
        pastedModelJson: cursorModelJson,
      })
      setRows([r])
      onBanner({ kind: 'success', text: `Committed: wiki/${r.detail ?? ''}` })
      setCursorModelJson('')
      setCursorPreview(null)
    } catch (e) {
      const msg = String(e)
      setIngestError(msg)
      onBanner({ kind: 'error', text: msg })
    } finally {
      unlisten()
      setCursorAssistBusy(false)
    }
  }

  const handleDiscoverWorkspaces = async () => {
    setArchiveBusy(true)
    setArchiveExcerpt(null)
    try {
      const hasFilters = !!(hashFilter.trim() || maxAgeDays.trim() || vaultFilter.trim())
      const query = hasFilters
        ? {
            hashContains: hashFilter.trim() || undefined,
            maxAgeDays: (() => {
              const t = maxAgeDays.trim()
              if (!t) return undefined
              const d = Number.parseInt(t, 10)
              return Number.isFinite(d) && d > 0 ? d : undefined
            })(),
            vaultPathContains: vaultFilter.trim() || undefined,
          }
        : undefined
      const list = await invoke<CursorWorkspaceEntry[]>('cursor_archive_discover_cmd', { query })
      setWorkspaces(list)
      if (!list.some((w) => w.absPath === wsSelected)) {
        setWsSelected('')
        setTranscripts([])
        setTxSelected('')
      }
      onBanner({ kind: 'success', text: `Found ${list.length} Cursor workspace folder(s).` })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    } finally {
      setArchiveBusy(false)
    }
  }

  const handleLoadTranscripts = async () => {
    if (!wsSelected) return
    setArchiveBusy(true)
    setArchiveExcerpt(null)
    try {
      const list = await invoke<CursorTranscriptFile[]>('cursor_archive_list_cmd', { workspaceAbs: wsSelected })
      setTranscripts(list)
      setTxSelected('')
      onBanner({ kind: 'success', text: `${list.length} .jsonl file(s) under workspace.` })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    } finally {
      setArchiveBusy(false)
    }
  }

  const handlePreviewArchiveExcerpt = async () => {
    if (!txSelected) return
    setArchiveBusy(true)
    try {
      const ex = await invoke<ChatExcerpt>('cursor_archive_preview_cmd', { path: txSelected, maxChars: 80_000 })
      setArchiveExcerpt(ex)
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
      setArchiveExcerpt(null)
    } finally {
      setArchiveBusy(false)
    }
  }

  const handleArchiveApplyDestinations = async () => {
    if (!cfg || !txSelected) {
      onBanner({ kind: 'error', text: 'Select a transcript file.' })
      return
    }
    if (!destEnrich && !destWiki && !destSession) {
      onBanner({ kind: 'error', text: 'Pick at least one destination.' })
      return
    }
    const parts: string[] = []
    if (destEnrich) parts.push('append excerpt to prompt pack')
    if (destWiki) parts.push('wiki source via ingest JSON')
    if (destSession) parts.push('new Lite chat session')
    if (!window.confirm(`Apply: ${parts.join(', ')}?`)) return

    setArchiveBusy(true)
    const unlisten = destWiki ? await subscribeIngestProgress() : null
    try {
      if (destEnrich) {
        const ex = archiveExcerpt ?? (await invoke<ChatExcerpt>('cursor_archive_preview_cmd', { path: txSelected, maxChars: 80_000 }))
        setArchiveExcerpt(ex)
        const block = `\n\n---\n\n## Archived Cursor excerpt (${ex.title})\n\nSource: \`${ex.sourcePath}\`\n\n${ex.messages.map((m) => `### ${m.role}\n\n${m.text}`).join('\n\n')}\n`
        setCursorPromptPack((prev) => (prev ? prev + block : block))
        setCursorAssistOpen(true)
        setShowPaste(true)
      }
      if (destWiki) {
        const r = await invoke<FileIngestResult>('cursor_archive_commit_excerpt_wiki_cmd', {
          cfg,
          transcriptPath: txSelected,
          maxChars: 80_000,
          trackId: resolvedTrackId,
          titleStem: undefined,
        })
        setRows([r])
        onBanner({ kind: 'success', text: `Wiki: ${r.detail ?? r.status}` })
      }
      if (destSession) {
        const sess = await invoke<SessionFile>('cursor_archive_import_excerpt_session_cmd', {
          transcriptPath: txSelected,
          maxChars: 80_000,
        })
        onBanner({ kind: 'success', text: `Lite session: ${sess.title}` })
      }
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    } finally {
      unlisten?.()
      setArchiveBusy(false)
    }
  }

  const handleRevealTranscript = async () => {
    if (!txSelected) return
    try {
      await invoke('cursor_archive_reveal_cmd', { path: txSelected })
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  return (
    <div className="w-full px-6 py-6 flex flex-col gap-5">
      {ingestError && (
        <div
          role="alert"
          className="rounded-[var(--radius-md)] border border-[var(--color-destructive)] bg-[var(--color-destructive)]/10 px-3 py-2 text-sm text-[var(--color-foreground)]"
        >
          <div className="flex items-start justify-between gap-2">
            <pre className="whitespace-pre-wrap break-words flex-1 font-mono text-xs leading-relaxed">{ingestError}</pre>
            <div className="flex shrink-0 gap-1">
              <CopyButton text={ingestError} label="Copy error" />
              <Button type="button" variant="ghost" size="sm" className="h-7 text-xs" onClick={() => setIngestError(null)}>
                Dismiss
              </Button>
            </div>
          </div>
        </div>
      )}

      <div>
        <h1 className="text-lg font-semibold text-[var(--color-foreground)]">Ingest</h1>
        <p className="text-sm text-[var(--color-muted-foreground)] mt-0.5">
          Scans <code className="text-xs bg-[var(--color-muted)] px-1 py-0.5 rounded">raw/</code> and builds structured wiki entries using your schema. LLM:{' '}
          <strong className="text-[var(--color-foreground)]">{llmLine.provider}</strong>
          {' · '}
          <strong className="text-[var(--color-foreground)]">{llmLine.model}</strong>
          {ingestHints ? (
            <>
              {' · '}
              <span className={ingestHints.visionCapable ? 'text-[var(--color-success)]' : 'text-[var(--color-muted-foreground)]'}>
                Vision ingest: {ingestHints.visionCapable ? 'supported' : 'not supported'} ({ingestHints.activeProvider}/{ingestHints.activeModelId})
              </span>
            </>
          ) : null}
        </p>
        {ingestHints && (
          <details className="mt-2 text-xs text-[var(--color-muted-foreground)] max-w-3xl">
            <summary className="cursor-pointer select-none text-[var(--color-foreground)]">Supported raw extensions ({ingestHints.supportedExtensions.length})</summary>
            <p className="mt-1.5 break-words">{ingestHints.supportedExtensions.join(', ')}</p>
          </details>
        )}
      </div>

      {/* Controls */}
      <Card>
        <CardContent className="pt-4 flex flex-col gap-4">
          <div className="flex items-center gap-3 flex-wrap">
            <Button onClick={handleRunIngest} disabled={busy || invalidTrackSelection} className="gap-2">
              {busy ? <Loader2 size={14} className="animate-spin" /> : <FileText size={14} />}
              {busy ? 'Ingesting…' : 'Run full ingest'}
            </Button>
            {busy && (
              <Button variant="destructive" onClick={handleStopIngest}>
                Stop ingest
              </Button>
            )}
            <Button variant="secondary" onClick={() => setShowPaste(!showPaste)} disabled={busy}>
              {showPaste ? 'Hide paste' : 'Paste & ingest'}
            </Button>
            <label className="flex items-center gap-2 text-sm text-[var(--color-foreground)] cursor-pointer select-none ml-auto">
              <input
                type="checkbox"
                checked={fullTier}
                onChange={(e) => setFullTier(e.target.checked)}
                className="rounded"
              />
              Full tier (richer prompts)
            </label>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>Track mode</Label>
              <select
                value={trackMode}
                onChange={(e) => setTrackMode(e.target.value as IngestTrackMode)}
                disabled={busy}
                className="h-9 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-background)] px-3 text-sm text-[var(--color-foreground)]"
              >
                <option value="auto">Auto-detect</option>
                <option value="existing">Existing track</option>
                <option value="new">Create new track</option>
              </select>
            </div>
            {trackMode === 'existing' && (
              <div className="flex flex-col gap-1.5 md:col-span-2">
                <Label>Select track</Label>
                <select
                  value={existingTrack}
                  onChange={(e) => setExistingTrack(e.target.value)}
                  disabled={busy}
                  className="h-9 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-background)] px-3 text-sm text-[var(--color-foreground)]"
                >
                  <option value="">Choose track…</option>
                  {tracks.map((t) => <option key={t} value={t}>{t}</option>)}
                </select>
              </div>
            )}
            {trackMode === 'new' && (
              <div className="flex flex-col gap-1.5 md:col-span-2">
                <Label>New track ID</Label>
                <Input
                  value={newTrack}
                  onChange={(e) => setNewTrack(e.target.value)}
                  disabled={busy}
                  placeholder="e.g. claims-modernization"
                />
              </div>
            )}
          </div>
          {trackMode === 'auto' && autoInference && (
            <p className="text-xs text-[var(--color-muted-foreground)]">
              Auto route: <strong className="text-[var(--color-foreground)]">{autoInference.trackId ?? 'inbox'}</strong>
              {' '}({Math.round(autoInference.confidence * 100)}% confidence)
              {autoInference.reason ? ` · ${autoInference.reason}` : ''}
            </p>
          )}
          <div className="border-t border-[var(--color-border)] pt-4 flex flex-col gap-3">
            <p className="text-xs text-[var(--color-muted-foreground)]">
              Ingest a web page into the selected track.
            </p>
            <div className="flex flex-col gap-1.5">
              <Label>URL</Label>
              <Input
                value={urlInput}
                onChange={(e) => setUrlInput(e.target.value)}
                placeholder="https://example.com/article"
                disabled={busy}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>Filename (optional)</Label>
              <Input
                value={urlStem}
                onChange={(e) => setUrlStem(e.target.value)}
                placeholder="e.g. architecture-notes"
                disabled={busy}
              />
            </div>
            <div className="flex items-center gap-2">
              <Button variant="secondary" onClick={handleUrlIngest} disabled={busy || !urlInput.trim() || invalidTrackSelection}>
                {busy ? <Loader2 size={14} className="animate-spin mr-1.5" /> : null}
                {busy ? 'Working…' : 'Fetch URL & ingest'}
              </Button>
            </div>
          </div>

          {showPaste && (
            <div className="border-t border-[var(--color-border)] pt-4 flex flex-col gap-3">
              <p className="text-xs text-[var(--color-muted-foreground)]">
                Saves as <code className="bg-[var(--color-muted)] px-1 py-0.5 rounded">raw/&lt;track&gt;/pastes/&lt;name&gt;.md</code>, then runs ingest.
              </p>
              <div className="flex flex-col gap-1.5">
                <Label>Filename (optional)</Label>
                <Input
                  value={pasteTitle}
                  onChange={(e) => setPasteTitle(e.target.value)}
                  placeholder="e.g. meeting-notes"
                  disabled={busy}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>Content</Label>
                <Textarea
                  value={pasteBody}
                  onChange={(e) => setPasteBody(e.target.value)}
                  placeholder="Paste notes, article text, transcript…"
                  disabled={busy}
                  className="min-h-40"
                />
              </div>
              <div className="flex items-center gap-2 flex-wrap">
                <Button variant="secondary" onClick={handlePasteIngest} disabled={busy || memoryBusy || !pasteBody.trim() || invalidTrackSelection}>
                  {busy ? <Loader2 size={14} className="animate-spin mr-1.5" /> : null}
                  {busy ? 'Working…' : 'Save to raw & ingest'}
                </Button>
                <Button variant="ghost" onClick={handleRollupToMemory} disabled={busy || memoryBusy || !pasteBody.trim()} className="gap-1.5">
                  {memoryBusy ? <Loader2 size={14} className="animate-spin" /> : <BrainCircuit size={14} />}
                  {memoryBusy ? 'Updating memory…' : 'Roll up to memory'}
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="py-3">
          <div className="flex items-center justify-between gap-2 flex-wrap">
            <CardTitle className="text-base">Cursor-assisted ingest</CardTitle>
            <label className="flex items-center gap-2 text-sm cursor-pointer select-none">
              <input
                type="checkbox"
                checked={cursorAssistOpen}
                onChange={(e) => setCursorAssistOpen(e.target.checked)}
                className="rounded"
              />
              Show workflow
            </label>
          </div>
          <p className="text-xs text-[var(--color-muted-foreground)] mt-1">
            For environments where only Cursor-hosted models are allowed: save to <code className="bg-[var(--color-muted)] px-1 rounded">raw/</code>, copy a prompt pack into Cursor Chat, paste JSON back — no provider API call from this app.
          </p>
        </CardHeader>
        {cursorAssistOpen && (
          <CardContent className="pt-0 flex flex-col gap-3 border-t border-[var(--color-border)]">
            <p className="text-xs text-[var(--color-muted-foreground)]">
              1) Fill <strong>Paste</strong> above (track + content). 2) Generate pack. 3) Paste model JSON. 4) Validate, then commit.
            </p>
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                variant="secondary"
                onClick={handlePrepareCursorAssist}
                disabled={busy || cursorAssistBusy || invalidTrackSelection || !pasteBody.trim()}
              >
                {cursorAssistBusy ? <Loader2 size={14} className="animate-spin mr-1.5" /> : null}
                Save to raw &amp; generate prompt pack
              </Button>
              {cursorRawRel && (
                <span className="text-xs font-mono text-[var(--color-muted-foreground)] self-center">
                  raw/{cursorRawRel}
                </span>
              )}
            </div>
            <div className="flex flex-col gap-1.5">
              <div className="flex items-center justify-between gap-2">
                <Label>Prompt pack (copy to Cursor Chat)</Label>
                {cursorPromptPack ? <CopyButton text={cursorPromptPack} label="Copy pack" /> : null}
              </div>
              <Textarea
                value={cursorPromptPack}
                onChange={(e) => setCursorPromptPack(e.target.value)}
                placeholder="Generate a pack after saving raw…"
                className="min-h-48 font-mono text-xs"
                disabled={cursorAssistBusy}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>Model response (JSON)</Label>
              <Textarea
                value={cursorModelJson}
                onChange={(e) => {
                  setCursorModelJson(e.target.value)
                  setCursorPreview(null)
                }}
                placeholder='Paste a single JSON object with slug, title, one_line_summary, body_markdown_b64, …'
                className="min-h-32 font-mono text-xs"
              />
            </div>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="secondary" onClick={handlePreviewCursorCommit} disabled={!cursorRawRel || !cursorModelJson.trim()}>
                Validate JSON
              </Button>
              <Button
                type="button"
                onClick={handleCommitCursorAssist}
                disabled={!cursorRawRel || !cursorModelJson.trim() || !cursorPreview || cursorAssistBusy}
              >
                Commit to wiki
              </Button>
            </div>
            {cursorPreview && (
              <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-muted)]/30 p-3 text-xs space-y-1">
                <div><strong>Title:</strong> {cursorPreview.title}</div>
                <div><strong>Slug:</strong> {cursorPreview.slug}</div>
                <div><strong>Summary:</strong> {cursorPreview.oneLineSummary}</div>
                <div><strong>Wiki file:</strong> {cursorPreview.wikiSourceRel}</div>
                <div><strong>Tags:</strong> {cursorPreview.tags.join(', ') || '—'}</div>
                <div><strong>Track:</strong> {cursorPreview.trackId ?? 'inbox'}</div>
              </div>
            )}
          </CardContent>
        )}
      </Card>

      <Card>
        <CardHeader className="py-3">
          <div className="flex items-center justify-between gap-2 flex-wrap">
            <CardTitle className="text-base">Import Cursor chat archive</CardTitle>
            <label className="flex items-center gap-2 text-sm cursor-pointer select-none">
              <input
                type="checkbox"
                checked={archiveOpen}
                onChange={(e) => setArchiveOpen(e.target.checked)}
                className="rounded"
              />
              Show local transcripts
            </label>
          </div>
          <p className="text-xs text-[var(--color-muted-foreground)] mt-1">
            Reads local Cursor <code className="bg-[var(--color-muted)] px-1 rounded">workspaceStorage</code> for <code className="bg-[var(--color-muted)] px-1 rounded">*.jsonl</code> only. <code className="bg-[var(--color-muted)] px-1 rounded">state.vscdb</code> is not parsed yet. Use &quot;Reveal in Finder&quot; for external tools.
          </p>
        </CardHeader>
        {archiveOpen && (
          <CardContent className="pt-0 flex flex-col gap-3 border-t border-[var(--color-border)]">
            <div className="grid grid-cols-1 md:grid-cols-3 gap-2">
              <div className="flex flex-col gap-1">
                <Label className="text-xs">Hash contains</Label>
                <Input value={hashFilter} onChange={(e) => setHashFilter(e.target.value)} placeholder="workspace hash" className="h-8 text-xs" />
              </div>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">Max age (days)</Label>
                <Input value={maxAgeDays} onChange={(e) => setMaxAgeDays(e.target.value)} placeholder="30" className="h-8 text-xs" />
              </div>
              <div className="flex flex-col gap-1">
                <Label className="text-xs">Vault path contains</Label>
                <Input value={vaultFilter} onChange={(e) => setVaultFilter(e.target.value)} placeholder="folder URI substring" className="h-8 text-xs" />
              </div>
            </div>
            <Button type="button" variant="secondary" size="sm" onClick={handleDiscoverWorkspaces} disabled={archiveBusy}>
              {archiveBusy ? <Loader2 size={14} className="animate-spin mr-1.5" /> : null}
              Discover workspaces
            </Button>
            <div className="flex flex-col gap-1.5">
              <Label>Workspace</Label>
              <select
                className="h-9 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-background)] px-2 text-sm"
                value={wsSelected}
                onChange={(e) => {
                  setWsSelected(e.target.value)
                  setTranscripts([])
                  setTxSelected('')
                  setArchiveExcerpt(null)
                }}
              >
                <option value="">Select…</option>
                {workspaces.map((w) => (
                  <option key={w.absPath} value={w.absPath}>
                    {w.projectSlug}
                    {w.folderHint ? ` — ${w.folderHint.slice(0, 80)}` : ''}
                    {w.hasStateVscdb ? ' [vscdb]' : ''}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="secondary" size="sm" onClick={handleLoadTranscripts} disabled={archiveBusy || !wsSelected}>
                List .jsonl transcripts
              </Button>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>Transcript file</Label>
              <select
                className="h-9 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-background)] px-2 text-sm font-mono text-xs"
                value={txSelected}
                onChange={(e) => {
                  setTxSelected(e.target.value)
                  setArchiveExcerpt(null)
                }}
              >
                <option value="">Select…</option>
                {transcripts.map((t) => (
                  <option key={t.absPath} value={t.absPath}>
                    {t.name} ({Math.round(t.sizeBytes / 1024)} KiB)
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" size="sm" onClick={handlePreviewArchiveExcerpt} disabled={archiveBusy || !txSelected}>
                Preview excerpt
              </Button>
              <Button type="button" variant="outline" size="sm" onClick={handleRevealTranscript} disabled={!txSelected}>
                <FolderOpen size={14} className="mr-1.5 inline" />
                Reveal in OS
              </Button>
              {txSelected ? <CopyButton text={txSelected} label="Copy path" /> : null}
            </div>
            <div className="flex flex-col gap-2 border border-[var(--color-border)] rounded-md p-2">
              <span className="text-xs font-medium text-[var(--color-foreground)]">Destinations (multi)</span>
              <label className="flex items-center gap-2 text-xs">
                <input type="checkbox" checked={destEnrich} onChange={(e) => setDestEnrich(e.target.checked)} />
                Append excerpt to Cursor-assisted prompt pack
              </label>
              <label className="flex items-center gap-2 text-xs">
                <input type="checkbox" checked={destWiki} onChange={(e) => setDestWiki(e.target.checked)} />
                Stage wiki page (stub ingest JSON, track from ingest controls)
              </label>
              <label className="flex items-center gap-2 text-xs">
                <input type="checkbox" checked={destSession} onChange={(e) => setDestSession(e.target.checked)} />
                Import as Lite chat session
              </label>
            </div>
            <Button type="button" onClick={handleArchiveApplyDestinations} disabled={archiveBusy || !txSelected}>
              {archiveBusy ? <Loader2 size={14} className="animate-spin mr-1.5" /> : null}
              Apply selected destinations
            </Button>
            {archiveExcerpt && (
              <div className="text-xs border border-[var(--color-border)] rounded-md p-2 max-h-40 overflow-y-auto">
                <strong>{archiveExcerpt.title}</strong> — {archiveExcerpt.messages.length} message(s)
                {archiveExcerpt.truncated ? ' (truncated)' : ''}
              </div>
            )}
          </CardContent>
        )}
      </Card>

      {/* Progress */}
      {(busy || cursorAssistBusy || archiveBusy || logLines.length > 0) && (
        <Card>
          <CardHeader className="py-2.5">
            <div className="flex items-center justify-between">
              <CardTitle className="text-xs uppercase tracking-widest text-[var(--color-muted-foreground)]">Progress</CardTitle>
              {logLines.length > 0 && <CopyButton text={logLines.join('\n')} label="Copy progress log" />}
            </div>
          </CardHeader>
          <CardContent className="pt-0 pb-3">
            <pre className="text-xs font-mono text-[var(--color-foreground)] leading-relaxed max-h-52 overflow-y-auto whitespace-pre-wrap break-words">
              {logLines.length ? logLines.join('\n') : 'Starting…'}
            </pre>
          </CardContent>
        </Card>
      )}

      {/* Results table */}
      {rows.length > 0 && (
        <Card>
          <CardHeader className="py-3">
            <CardTitle>Results — {rows.length} files</CardTitle>
          </CardHeader>
          <div className="overflow-x-auto">
            <table className="w-full text-xs border-collapse">
              <thead>
                <tr className="border-b border-[var(--color-border)]">
                  <th className="text-left px-4 py-2 text-[var(--color-muted-foreground)] font-medium w-8"></th>
                  <th className="text-left px-4 py-2 text-[var(--color-muted-foreground)] font-medium">File</th>
                  <th className="text-left px-4 py-2 text-[var(--color-muted-foreground)] font-medium w-20">Status</th>
                  <th className="text-left px-4 py-2 text-[var(--color-muted-foreground)] font-medium">Detail</th>
                  <th className="px-2 py-2 w-8"></th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.relativeRawPath} className="border-b border-[var(--color-border)] last:border-0 hover:bg-[var(--color-muted)] transition-colors">
                    <td className="px-4 py-2"><StatusIcon status={r.status} /></td>
                    <td className="px-4 py-2 font-mono text-[var(--color-foreground)]">{r.relativeRawPath}</td>
                    <td className={cn('px-4 py-2 font-medium', r.status === 'ok' ? 'text-[var(--color-success)]' : r.status === 'skipped' || r.status === 'cancelled' ? 'text-[var(--color-muted-foreground)]' : 'text-[var(--color-destructive)]')}>
                      {r.status}
                    </td>
                    <td className="px-4 py-2 text-[var(--color-muted-foreground)]">{r.detail ?? ''}</td>
                    <td className="px-2 py-2">
                      {r.status === 'error' && (
                        <CopyButton
                          text={[r.relativeRawPath, r.status, r.detail ?? ''].filter(Boolean).join('\n')}
                          label="Copy error"
                        />
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}

      {!busy && rows.length === 0 && logLines.length === 0 && (
        <p className="text-sm text-[var(--color-muted-foreground)] text-center py-8">
          Run ingest to see results here.
        </p>
      )}
    </div>
  )
}
