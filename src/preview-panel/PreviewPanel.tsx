import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { Loader2 } from 'lucide-react'
import React, { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { resolveUcUrl } from '@/lib/protocol'
import { applyThemePreset, DEFAULT_THEME_COLOR } from '@/lib/theme-engine'
import type { ThemeMode } from '@/lib/theme-engine'
import type { SettingChangedEvent } from '@/types/events'
import type { Settings } from '@/types/setting'

// ── Types ──────────────────────────────────────────────────────────────

interface ClipboardEntryDetail {
  id: string
  content: string
  content_type: string
  size_bytes: number
  is_favorited: boolean
  updated_at: number
  active_time: number
}

interface ClipboardEntryResource {
  blob_id: string | null
  mime_type: string
  size_bytes: number
  url: string | null
  inline_data: string | null
}

interface ShowPayload {
  entryId: string
}

// ── Theme sync ─────────────────────────────────────────────────────────

function resolveThemeMode(theme: string | undefined | null): ThemeMode {
  if (theme === 'light' || theme === 'dark') return theme
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function applyFullTheme(settings: Settings | null): void {
  const root = document.documentElement
  const theme = settings?.general?.theme
  const themeColor = settings?.general?.theme_color || DEFAULT_THEME_COLOR

  const resolvedMode = resolveThemeMode(theme)
  root.classList.remove('light', 'dark')
  root.classList.add(resolvedMode)
  applyThemePreset(themeColor, resolvedMode, root)
}

// ── Helpers ────────────────────────────────────────────────────────────

function isImageType(contentType: string): boolean {
  return contentType === 'image' || contentType.startsWith('image/')
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

// ── Component ──────────────────────────────────────────────────────────

function getResourceImageUrl(resource: ClipboardEntryResource): string | null {
  if (resource.url) {
    return resolveUcUrl(resource.url)
  }
  if (resource.inline_data) {
    return `data:${resource.mime_type};base64,${resource.inline_data}`
  }
  return null
}

// Unified preview state
interface PreviewState {
  entryId: string
  contentType: 'text' | 'image'
  sizeBytes: number
  // Text content (only for text type)
  textContent?: string
  // Image URL (only for image type)
  imageUrl?: string
}

const PreviewPanel: React.FC = () => {
  const { t } = useTranslation(undefined, { keyPrefix: 'previewPanel' })
  const [preview, setPreview] = useState<PreviewState | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const requestIdRef = useRef(0)
  const settingsRef = useRef<Settings | null>(null)
  const isMac = useMemo(() => navigator.platform.toUpperCase().includes('MAC'), [])

  // ── Theme sync ──
  useEffect(() => {
    async function loadAndApplyTheme() {
      try {
        const settings = await invoke<Settings>('get_settings')
        settingsRef.current = settings
        applyFullTheme(settings)
      } catch (err) {
        console.error('Failed to load settings for theme:', err)
        applyFullTheme(null)
      }
    }

    loadAndApplyTheme()

    const unlistenSettings = listen<SettingChangedEvent>('setting-changed', event => {
      try {
        const newSettings = JSON.parse(event.payload.settingJson) as Settings
        settingsRef.current = newSettings
        applyFullTheme(newSettings)
      } catch (err) {
        console.error('Failed to parse setting-changed event:', err)
      }
    })

    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const handleSystemChange = () => {
      const settings = settingsRef.current
      if (!settings?.general?.theme || settings.general.theme === 'system') {
        applyFullTheme(settings)
      }
    }
    mq.addEventListener('change', handleSystemChange)

    return () => {
      unlistenSettings.then(fn => fn())
      mq.removeEventListener('change', handleSystemChange)
    }
  }, [])

  // ── Event listeners ──
  useEffect(() => {
    const unlistenShow = listen<ShowPayload>('preview-panel://show', async event => {
      const { entryId } = event.payload
      const currentRequestId = ++requestIdRef.current
      setLoading(true)
      setError(null)
      setPreview(null)

      try {
        // First, get resource metadata (works for ALL content types)
        const resource = await invoke<ClipboardEntryResource>('get_clipboard_entry_resource', {
          entryId,
        })

        if (currentRequestId !== requestIdRef.current) return

        if (isImageType(resource.mime_type)) {
          // Image: use resource URL directly (get_clipboard_entry_detail fails for images)
          const url = getResourceImageUrl(resource)
          setPreview({
            entryId,
            contentType: 'image',
            sizeBytes: resource.size_bytes,
            imageUrl: url ?? undefined,
          })
        } else {
          // Text: use get_clipboard_entry_detail for full text content
          const detail = await invoke<ClipboardEntryDetail>('get_clipboard_entry_detail', {
            entryId,
          })

          if (currentRequestId !== requestIdRef.current) return

          setPreview({
            entryId,
            contentType: 'text',
            sizeBytes: detail.size_bytes,
            textContent: detail.content,
          })
        }
      } catch (err) {
        if (currentRequestId !== requestIdRef.current) return
        console.error('Failed to load preview:', err)
        setError(String(err))
      } finally {
        if (currentRequestId === requestIdRef.current) {
          setLoading(false)
        }
      }
    })

    const unlistenHide = listen('preview-panel://hide', () => {
      requestIdRef.current++
      setPreview(null)
      setError(null)
      setLoading(false)
    })

    return () => {
      unlistenShow.then(fn => fn())
      unlistenHide.then(fn => fn())
    }
  }, [])

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden rounded-xl bg-background/95 backdrop-blur-xl shadow-xl border border-border/50">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border/50">
        <span className="text-[12px] font-medium text-foreground">{t('title')}</span>
        {preview && (
          <span className="text-[11px] text-muted-foreground tabular-nums">
            {formatBytes(preview.sizeBytes)}
          </span>
        )}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-auto px-3 py-2">
        {loading ? (
          <div className="flex items-center justify-center h-full">
            <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
          </div>
        ) : error ? (
          <div className="flex items-center justify-center h-full text-[12px] text-destructive">
            {t('error')}
          </div>
        ) : preview ? (
          preview.contentType === 'image' ? (
            <div className="flex items-center justify-center h-full">
              {preview.imageUrl ? (
                <img
                  src={preview.imageUrl}
                  className="max-w-full max-h-full object-contain rounded-md"
                  alt={t('imageAlt')}
                />
              ) : (
                <span className="text-[12px] text-muted-foreground">{t('imageUnavailable')}</span>
              )}
            </div>
          ) : (
            <pre className="text-[12px] leading-relaxed text-foreground whitespace-pre-wrap break-words select-text cursor-text font-mono">
              {preview.textContent}
            </pre>
          )
        ) : (
          <div className="flex items-center justify-center h-full text-[12px] text-muted-foreground">
            {t('empty')}
          </div>
        )}
      </div>

      {/* Footer hint */}
      <div className="flex items-center justify-start px-3 py-1.5 border-t border-border/50 text-[11px] text-muted-foreground">
        <span>{t('deleteHint', { modifier: isMac ? '⌥' : 'Alt+' })}</span>
      </div>
    </div>
  )
}

export default PreviewPanel
