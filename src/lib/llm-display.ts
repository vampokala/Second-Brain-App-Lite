import type { AppConfig } from '@/types'

/** Align with `config::normalize_llm_provider` (Rust). */
export function normalizeLlmProvider(raw: string): string {
  const p = raw.trim().toLowerCase()
  if (!p) return 'ollama'
  if (p === 'claude') return 'anthropic'
  if (p === 'google') return 'gemini'
  return p
}

/** Model id used for the given provider, matching `llm.rs` routing (raw cfg field). */
export function modelIdForNormalizedProvider(cfg: AppConfig, provider: string): string {
  switch (provider) {
    case 'ollama':
      return cfg.ollamaModel
    case 'openai':
      return cfg.openaiModel
    case 'anthropic':
      return cfg.anthropicModel
    case 'gemini':
      return cfg.geminiModel ?? ''
    case 'compatible':
      return cfg.compatibleModel
    default:
      return ''
  }
}

export function ingestLlmSummary(cfg: AppConfig): { provider: string; model: string } {
  const provider = normalizeLlmProvider(cfg.defaultProvider)
  const raw = modelIdForNormalizedProvider(cfg, provider).trim()
  const model = raw || '—'
  return { provider, model }
}
