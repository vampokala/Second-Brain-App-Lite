import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useCallback, useState } from 'react'
import type { AppConfig, FileIngestResult, IngestProgressPayload, TrackInference } from '@/types'

function formatLine(p: IngestProgressPayload): string {
  const step = p.current != null && p.total != null ? `[${p.current}/${p.total}] ` : ''
  const path = p.relativePath ? `${p.relativePath} · ` : ''
  return `${step}${path}${p.phase}: ${p.message}`
}

export function useIngest() {
  const [busy, setBusy] = useState(false)
  const [rows, setRows] = useState<FileIngestResult[]>([])
  const [logLines, setLogLines] = useState<string[]>([])

  const reset = () => { setRows([]); setLogLines([]) }

  const subscribe = async () =>
    listen<IngestProgressPayload>('ingest-progress', (ev) => {
      setLogLines((prev) => [...prev, formatLine(ev.payload)])
    })

  const runIngest = async (cfg: AppConfig, fullTier: boolean, trackId?: string): Promise<FileIngestResult[]> => {
    setBusy(true)
    reset()
    const unlisten = await subscribe()
    try {
      const result = await invoke<FileIngestResult[]>('run_ingest_cmd', {
        cfg,
        fullTier,
        trackId: trackId?.trim() || undefined,
      })
      setRows(result)
      return result
    } finally {
      unlisten()
      setBusy(false)
    }
  }

  const pasteAndIngest = async (
    cfg: AppConfig,
    fullTier: boolean,
    pasteBody: string,
    pasteTitle?: string,
    trackId?: string,
    autoDetectTrack?: boolean,
  ): Promise<FileIngestResult[]> => {
    setBusy(true)
    reset()
    const unlisten = await subscribe()
    try {
      const result = await invoke<FileIngestResult[]>('ingest_pasted_text_cmd', {
        cfg,
        fullTier,
        payload: {
          content: pasteBody,
          fileStem: pasteTitle?.trim() || undefined,
          trackId: trackId?.trim() || undefined,
          autoDetectTrack: !!autoDetectTrack,
        },
      })
      setRows(result)
      return result
    } finally {
      unlisten()
      setBusy(false)
    }
  }

  const ingestUrl = async (
    cfg: AppConfig,
    fullTier: boolean,
    url: string,
    fileStem?: string,
    trackId?: string,
    autoDetectTrack?: boolean,
  ): Promise<FileIngestResult[]> => {
    setBusy(true)
    reset()
    const unlisten = await subscribe()
    try {
      const result = await invoke<FileIngestResult[]>('ingest_url_cmd', {
        cfg,
        fullTier,
        payload: {
          url: url.trim(),
          fileStem: fileStem?.trim() || undefined,
          trackId: trackId?.trim() || undefined,
          autoDetectTrack: !!autoDetectTrack,
        },
      })
      setRows(result)
      return result
    } finally {
      unlisten()
      setBusy(false)
    }
  }

  const listTracks = useCallback(async (cfg: AppConfig): Promise<string[]> =>
    invoke<string[]>('list_tracks_cmd', { cfg }), [])

  const inferTrack = useCallback(
    async (cfg: AppConfig, content: string, hint?: string): Promise<TrackInference> =>
      invoke<TrackInference>('infer_track_cmd', {
        cfg,
        payload: {
          content,
          hint: hint?.trim() || undefined,
        },
      }),
    [],
  )

  const cancelIngest = useCallback(
    async (): Promise<void> => {
      await invoke('cancel_ingest_cmd')
    },
    [],
  )

  return { busy, rows, logLines, runIngest, pasteAndIngest, ingestUrl, listTracks, inferTrack, cancelIngest }
}
