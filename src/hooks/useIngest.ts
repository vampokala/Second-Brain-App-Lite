import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useState } from 'react'
import type { AppConfig, FileIngestResult, IngestProgressPayload } from '@/types'

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

  const runIngest = async (cfg: AppConfig, fullTier: boolean): Promise<FileIngestResult[]> => {
    setBusy(true)
    reset()
    const unlisten = await subscribe()
    try {
      const result = await invoke<FileIngestResult[]>('run_ingest_cmd', { cfg, fullTier })
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
  ): Promise<FileIngestResult[]> => {
    setBusy(true)
    reset()
    const unlisten = await subscribe()
    try {
      const result = await invoke<FileIngestResult[]>('ingest_pasted_text_cmd', {
        cfg,
        fullTier,
        payload: { content: pasteBody, fileStem: pasteTitle?.trim() || undefined },
      })
      setRows(result)
      return result
    } finally {
      unlisten()
      setBusy(false)
    }
  }

  return { busy, rows, logLines, runIngest, pasteAndIngest }
}
