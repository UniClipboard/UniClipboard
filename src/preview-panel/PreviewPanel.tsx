import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { Loader2 } from 'lucide-react'
import React, { useEffect, useRef, useState } from 'react'
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

const PreviewPanel: React.FC = () => {
  const [detail, setDetail] = useState<ClipboardEntryDetail | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const settingsRef = useRef<Settings | null>(null)

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
      setLoading(true)
      setError(null)

      try {
        const result = await invoke<ClipboardEntryDetail>('get_clipboard_entry_detail', { entryId })
        setDetail(result)
      } catch (err) {
        console.error('Failed to load entry detail:', err)
        setError(String(err))
        setDetail(null)
      } finally {
        setLoading(false)
      }
    })

    const unlistenHide = listen('preview-panel://hide', () => {
      setDetail(null)
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
        <span className="text-[12px] font-medium text-foreground">Preview</span>
        {detail && (
          <span className="text-[11px] text-muted-foreground tabular-nums">
            {formatBytes(detail.size_bytes)}
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
            Failed to load preview
          </div>
        ) : detail ? (
          isImageType(detail.content_type) ? (
            <div className="flex items-center justify-center h-full">
              <span className="text-[12px] text-muted-foreground">Image preview not available</span>
            </div>
          ) : (
            <pre className="text-[12px] leading-relaxed text-foreground whitespace-pre-wrap break-words select-text cursor-text font-mono">
              {detail.content}
            </pre>
          )
        ) : (
          <div className="flex items-center justify-center h-full text-[12px] text-muted-foreground">
            Hover over an item to preview
          </div>
        )}
      </div>
    </div>
  )
}

export default PreviewPanel
