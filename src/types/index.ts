export type AppConfig = {
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
  retrievalTrackFilter?: string | null
  braveSearchCount?: number
  braveFetchMaxUrls?: number
  braveFetchTimeoutSecs?: number
  bravePageMaxChars?: number
  braveMaxBodyBytes?: number
  chatPersona?: string
  studentGrade?: string
  personaPromptAddon?: string
}

export type ChatRetrievalMeta = {
  wikiSourcesOnly: boolean
  includeWebSearch: boolean
  hitCount: number
  maxScore: number
  braveKeyConfigured: boolean
  webPagesFetched: number
  /** Human-readable persona for this completed send (from server). */
  personaDisplay?: string
  personaAddonApplied?: boolean
}

export type SessionFile = {
  id: string
  title: string
  created: string
  updated: string
  messages: { role: string; content: string; ts?: string }[]
}

export type FileIngestResult = {
  relativeRawPath: string
  status: string
  detail?: string | null
}

export type IngestProgressPayload = {
  phase: string
  message: string
  current?: number
  total?: number
  relativePath?: string
}

export type IngestTrackMode = 'existing' | 'new' | 'auto'

export type TrackInference = {
  trackId?: string | null
  confidence: number
  reason: string
}

export type SchemaStatus = {
  claudeMd: boolean
  llmWikiMd: boolean
}
