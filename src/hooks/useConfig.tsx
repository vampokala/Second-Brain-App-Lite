import type { AppConfig, SchemaStatus } from '@/types'
import { invoke } from '@tauri-apps/api/core'
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'

type ConfigContextValue = {
  cfg: AppConfig | null
  setCfg: React.Dispatch<React.SetStateAction<AppConfig | null>>
  patchCfg: (partial: Partial<AppConfig>) => void
  persistCfg: (next: AppConfig) => Promise<void>
  schemaStatus: SchemaStatus | null
  platformOs: string
  hints: Record<string, string | undefined>
  refreshHints: () => Promise<void>
  refreshSchema: (c: AppConfig) => Promise<void>
  error: string | null
}

const ConfigContext = createContext<ConfigContextValue | null>(null)

export function ConfigProvider({ children }: { children: ReactNode }) {
  const [cfg, setCfg] = useState<AppConfig | null>(null)
  const [schemaStatus, setSchemaStatus] = useState<SchemaStatus | null>(null)
  const [platformOs, setPlatformOs] = useState<string>('')
  const [hints, setHints] = useState<Record<string, string | undefined>>({})
  const [error, setError] = useState<string | null>(null)

  const refreshHints = useCallback(async () => {
    try {
      const [o, a, g, c, b] = await Promise.all([
        invoke<string | null>('api_secret_hint', { provider: 'openai' }),
        invoke<string | null>('api_secret_hint', { provider: 'anthropic' }),
        invoke<string | null>('api_secret_hint', { provider: 'gemini' }),
        invoke<string | null>('api_secret_hint', { provider: 'compatible' }),
        invoke<string | null>('api_secret_hint', { provider: 'brave' }),
      ])
      setHints({
        openai: o ?? undefined,
        anthropic: a ?? undefined,
        gemini: g ?? undefined,
        compatible: c ?? undefined,
        brave: b ?? undefined,
      })
    } catch {
      /* ignore */
    }
  }, [])

  const refreshSchema = useCallback(async (c: AppConfig) => {
    if (!c.schemaDir) {
      setSchemaStatus(null)
      return
    }
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

  const patchCfg = useCallback((partial: Partial<AppConfig>) => {
    setCfg((prev) => (prev ? { ...prev, ...partial } : prev))
  }, [])

  const persistCfg = useCallback(
    async (next: AppConfig): Promise<void> => {
      await invoke('save_app_config', { cfg: next })
      setCfg(next)
      refreshSchema(next)
    },
    [refreshSchema],
  )

  const value = useMemo<ConfigContextValue>(
    () => ({
      cfg,
      setCfg,
      patchCfg,
      persistCfg,
      schemaStatus,
      platformOs,
      hints,
      refreshHints,
      refreshSchema,
      error,
    }),
    [cfg, patchCfg, persistCfg, schemaStatus, platformOs, hints, refreshHints, refreshSchema, error],
  )

  return <ConfigContext.Provider value={value}>{children}</ConfigContext.Provider>
}

export function useConfig(): ConfigContextValue {
  const ctx = useContext(ConfigContext)
  if (!ctx) {
    throw new Error('useConfig must be used within ConfigProvider')
  }
  return ctx
}
