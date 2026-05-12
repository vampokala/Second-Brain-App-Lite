import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { CopyButton } from '@/components/ui/copy-button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useIngest } from '@/hooks/useIngest'
import { ingestLlmSummary } from '@/lib/llm-display'
import { cn } from '@/lib/utils'
import type { AppConfig, IngestTrackMode, TrackInference } from '@/types'
import { invoke } from '@tauri-apps/api/core'
import { BrainCircuit, CheckCircle2, FileText, Loader2, SkipForward, XCircle } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

type Banner = { kind: 'success' | 'error'; text: string } | null

function StatusIcon({ status }: { status: string }) {
  if (status === 'ok') return <CheckCircle2 size={14} className="text-[var(--color-success)] shrink-0" />
  if (status === 'skipped' || status === 'cancelled') return <SkipForward size={14} className="text-[var(--color-muted-foreground)] shrink-0" />
  return <XCircle size={14} className="text-[var(--color-destructive)] shrink-0" />
}

export default function IngestView({ cfg, onBanner }: { cfg: AppConfig | null; onBanner: (b: Banner) => void }) {
  const { busy, rows, logLines, runIngest, pasteAndIngest, ingestUrl, listTracks, inferTrack, cancelIngest } = useIngest()
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

  useEffect(() => {
    if (!cfg) return
    listTracks(cfg).then(setTracks).catch(() => setTracks([]))
  }, [cfg, listTracks])

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
    try {
      const result = await runIngest(cfg, fullTier, resolvedTrackId)
      const errCount = result.filter((r) => r.status === 'error').length
      if (errCount > 0) {
        onBanner({ kind: 'error', text: `Ingest finished with ${errCount} error(s).` })
      } else {
        onBanner({ kind: 'success', text: `Ingest finished — ${result.length} files scanned.` })
      }
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
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
    try {
      const result = await pasteAndIngest(cfg, fullTier, pasteBody, pasteTitle, resolvedTrackId, trackMode === 'auto')
      const errCount = result.filter((r) => r.status === 'error').length
      if (errCount > 0) {
        onBanner({ kind: 'error', text: `Ingest finished with ${errCount} error(s).` })
      } else {
        setPasteBody('')
        onBanner({ kind: 'success', text: `Saved to raw/<track>/pastes and ingested (${result.length} files).` })
        listTracks(cfg).then(setTracks).catch(() => undefined)
      }
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  const handleUrlIngest = async () => {
    if (!urlInput.trim()) { onBanner({ kind: 'error', text: 'Enter a URL to ingest.' }); return }
    if (!maybeWarnLowConfidence()) return
    try {
      const result = await ingestUrl(cfg, fullTier, urlInput, urlStem, resolvedTrackId, trackMode === 'auto')
      const errCount = result.filter((r) => r.status === 'error').length
      if (errCount > 0) {
        onBanner({ kind: 'error', text: `Ingest finished with ${errCount} error(s).` })
      } else {
        setUrlInput('')
        setUrlStem('')
        onBanner({ kind: 'success', text: `URL fetched and ingested (${result.length} files).` })
        listTracks(cfg).then(setTracks).catch(() => undefined)
      }
    } catch (e) {
      onBanner({ kind: 'error', text: String(e) })
    }
  }

  return (
    <div className="w-full px-6 py-6 flex flex-col gap-5">
      <div>
        <h1 className="text-lg font-semibold text-[var(--color-foreground)]">Ingest</h1>
        <p className="text-sm text-[var(--color-muted-foreground)] mt-0.5">
          Scans <code className="text-xs bg-[var(--color-muted)] px-1 py-0.5 rounded">raw/</code> and builds structured wiki entries using your schema. LLM:{' '}
          <strong className="text-[var(--color-foreground)]">{llmLine.provider}</strong>
          {' · '}
          <strong className="text-[var(--color-foreground)]">{llmLine.model}</strong>
        </p>
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

      {/* Progress */}
      {(busy || logLines.length > 0) && (
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
