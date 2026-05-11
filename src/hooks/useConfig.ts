import { invoke } from '@tauri-apps/api/core'
import { useCallback, useEffect, useState } from 'react'
import type { AppConfig, SchemaStatus } from '@/types'

export function useConfig() {
  const [cfg, setCfg] = useState<AppConfig | null>(null)
  const [schemaStatus, setSchemaStatus] = useState<SchemaStatus | null>(null)
  const [platformOs, setPlatformOs] = useState<string>('')
  const [hints, setHints] = useState<Record<string, string | undefined>>({})
  const [error, setError] = useState<string | null>(null)

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
    } catch { /* ignore */ }
  }, [])

  const refreshSchema = useCallback(async (c: AppConfig) => {
    if (!c.schemaDir) { setSchemaStatus(null); return }
    try {
      const st = await invoke<SchemaStatus>('read_schema_status', { schemaDir: c.schemaDir })
      setSchemaStatus(st)
    } catch {
      setSchemaStatus(null)
    }
  }, [])

  useEffect(() => {
    invoke<string>('get_platform_os').then(setPlatformOs).catch(() => setPlatformOs(''))
    invoke<AppConfig>('load_app_config')
      .then((c) => {
        setCfg(c)
        refreshSchema(c)
        refreshHints()
      })
      .catch(() => setError('Could not load configuration.'))
  }, [refreshHints, refreshSchema])

  const patchCfg = (partial: Partial<AppConfig>) =>
    setCfg((prev) => (prev ? { ...prev, ...partial } : prev))

  const persistCfg = async (next: AppConfig): Promise<void> => {
    await invoke('save_app_config', { cfg: next })
    setCfg(next)
    refreshSchema(next)
  }

  return { cfg, setCfg, patchCfg, persistCfg, schemaStatus, platformOs, hints, refreshHints, refreshSchema, error }
}
