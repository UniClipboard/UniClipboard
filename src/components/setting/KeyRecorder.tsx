'use client'

import { useEffect, useMemo, useState } from 'react'
import { useRecordHotkeys } from 'react-hotkeys-hook'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui'
import {
  getCandidateKeyIssues,
  resolveShortcuts,
  type ShortcutKeyOverrides,
} from '@/shortcuts/conflicts'
import { SHORTCUT_DEFINITIONS, type ShortcutScope } from '@/shortcuts/definitions'
import { normalizeHotkey } from '@/shortcuts/normalize'

interface KeyRecorderProps {
  shortcutId: string
  scope: ShortcutScope
  currentOverrides: ShortcutKeyOverrides
  onConfirm: (key: string, clearedIds?: string[]) => void
  onCancel: () => void
}

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)

/**
 * Format a modifier key for display using platform-appropriate symbols.
 */
function formatKeyPart(part: string): string {
  const lower = part.toLowerCase().trim()

  if (isMac) {
    switch (lower) {
      case 'mod':
      case 'meta':
      case 'cmd':
      case 'command':
        return '\u2318'
      case 'alt':
      case 'option':
        return '\u2325'
      case 'shift':
        return '\u21E7'
      case 'ctrl':
      case 'control':
        return '\u2303'
      default:
        return part.charAt(0).toUpperCase() + part.slice(1)
    }
  } else {
    switch (lower) {
      case 'mod':
      case 'meta':
      case 'cmd':
      case 'command':
      case 'ctrl':
      case 'control':
        return 'Ctrl'
      case 'alt':
      case 'option':
        return 'Alt'
      case 'shift':
        return 'Shift'
      default:
        return part.charAt(0).toUpperCase() + part.slice(1)
    }
  }
}

export function KeyRecorder({
  shortcutId,
  scope,
  currentOverrides,
  onConfirm,
  onCancel,
}: KeyRecorderProps) {
  const { t } = useTranslation()
  const [keys, { start, stop }] = useRecordHotkeys()
  const [recordedKey, setRecordedKey] = useState<string | null>(null)
  const [normalizedKey, setNormalizedKey] = useState<string | null>(null)

  // Resolve current shortcuts for conflict detection
  const resolvedShortcuts = useMemo(
    () => resolveShortcuts(SHORTCUT_DEFINITIONS, currentOverrides),
    [currentOverrides]
  )

  // Analyze candidate key for conflicts
  const issues = useMemo(() => {
    if (!normalizedKey) return []
    return getCandidateKeyIssues(resolvedShortcuts, {
      id: shortcutId,
      scope,
      key: normalizedKey,
    })
  }, [normalizedKey, resolvedShortcuts, shortcutId, scope])

  const errorIssue = issues.find(i => i.level === 'error')
  const warningIssues = issues.filter(i => i.level === 'warning')
  const infoIssues = issues.filter(i => i.level === 'info')

  // Start recording on mount
  useEffect(() => {
    start()
  }, [start])

  // Update recorded key when keys change
  useEffect(() => {
    if (keys.size === 0) {
      setRecordedKey(null)
      setNormalizedKey(null)
      return
    }

    // Check for Escape key first - cancels recording
    if (keys.has('escape')) {
      stop()
      onCancel()
      return
    }

    // Convert Set to hotkey string
    const keyArray = Array.from(keys)
    const joined = keyArray.join('+')
    setRecordedKey(joined)

    // Normalize for display and comparison
    const normalized = normalizeHotkey(joined)
    setNormalizedKey(normalized)
  }, [keys, stop, onCancel])

  const handleConfirm = () => {
    if (!normalizedKey) return
    stop()

    // If there's an error-level conflict, we need to clear those conflicting shortcuts
    const clearedIds = errorIssue?.relatedIds
    onConfirm(normalizedKey, clearedIds)
  }

  const handleCancelClick = () => {
    stop()
    onCancel()
  }

  const keyParts = recordedKey ? recordedKey.split('+').map(formatKeyPart) : []

  return (
    <div className="flex flex-col gap-2 p-3 rounded-md border-2 border-primary/50 bg-background animate-pulse">
      <div className="flex items-center gap-2">
        {recordedKey ? (
          <div className="flex items-center gap-0.5">
            {keyParts.map((part, idx) => (
              <span key={idx} className="flex items-center">
                {idx > 0 && <span className="text-muted-foreground text-xs mx-0.5">+</span>}
                <kbd className="bg-muted text-xs font-mono px-1.5 py-0.5 rounded border border-border/60 text-foreground">
                  {part}
                </kbd>
              </span>
            ))}
          </div>
        ) : (
          <span className="text-sm text-muted-foreground">{t('settings.shortcuts.recording')}</span>
        )}
      </div>

      {/* Conflict warnings */}
      {issues.length > 0 && (
        <div className="flex flex-col gap-1 text-xs">
          {errorIssue && (
            <div className="flex items-center gap-2 text-destructive">
              <span>{errorIssue.message}</span>
            </div>
          )}
          {warningIssues.map((issue, idx) => (
            <div key={idx} className="flex items-center gap-2 text-yellow-600 dark:text-yellow-400">
              <span>{issue.message}</span>
            </div>
          ))}
          {infoIssues.map((issue, idx) => (
            <div key={idx} className="flex items-center gap-2 text-muted-foreground">
              <span>{issue.message}</span>
            </div>
          ))}
        </div>
      )}

      {/* Action buttons */}
      <div className="flex items-center gap-2 mt-1">
        <Button
          size="sm"
          variant={errorIssue ? 'default' : 'outline'}
          onClick={handleConfirm}
          disabled={!recordedKey}
        >
          {errorIssue ? t('settings.shortcuts.confirmOverride') : t('settings.shortcuts.confirm')}
        </Button>
        <Button size="sm" variant="ghost" onClick={handleCancelClick}>
          {t('settings.shortcuts.cancel')}
        </Button>
      </div>
    </div>
  )
}
