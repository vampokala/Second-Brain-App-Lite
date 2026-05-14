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
  /** When false, PNG/JPEG/etc. raw files are skipped (no vision call). */
  visionEnabled?: boolean
  visionMaxBytes?: number
  visionMaxEdgePx?: number
  textMaxBytes?: number
  tabularMaxRows?: number
}

export type IngestUiHints = {
  supportedExtensions: string[]
  visionCapable: boolean
  activeProvider: string
  activeModelId: string
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

export type PrepareCursorAssistResponse = {
  rawRel: string
  promptPack: string
}

export type IngestCommitPreview = {
  title: string
  slug: string
  oneLineSummary: string
  tags: string[]
  trackId?: string | null
  wikiSourceRel: string
  rawRelativePath: string
}

export type CursorWorkspaceEntry = {
  id: string
  workspaceHash: string
  projectSlug: string
  absPath: string
  modifiedMs?: number | null
  folderHint?: string | null
  hasStateVscdb: boolean
}

export type CursorTranscriptFile = {
  name: string
  absPath: string
  modifiedMs?: number | null
  sizeBytes: number
}

export type ChatExcerpt = {
  title: string
  sourcePath: string
  messages: { role: string; text: string }[]
  truncated: boolean
}

export type SchemaStatus = {
  claudeMd: boolean
  llmWikiMd: boolean
}
