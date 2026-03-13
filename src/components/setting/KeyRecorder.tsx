'use client'

import { useEffect, useMemo, useState } from 'react'
import { useRecordHotkeys } from 'react-hotkeys-hook'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui'
import { formatKeyPart } from '@/lib/shortcut-format'
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

  // Start recording on mount, stop on unmount
  useEffect(() => {
    start()
    return () => stop()
  }, [start, stop])

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
    <div className="flex flex-col gap-2 p-3 rounded-md border-2 border-primary/50 bg-card animate-pulse">
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
          <span className="text-sm text-muted-foreground">
            {t('settings.sections.shortcuts.recording')}
          </span>
        )}
      </div>

      {/* Conflict warnings */}
      {issues.length > 0 && (
        <div className="flex flex-col gap-1 text-xs">
          {errorIssue && (
            <div className="flex items-center gap-2 text-destructive">
              <span>{t(errorIssue.messageKey, errorIssue.messageParams)}</span>
            </div>
          )}
          {warningIssues.map((issue, idx) => (
            <div key={idx} className="flex items-center gap-2 text-yellow-600 dark:text-yellow-400">
              <span>{t(issue.messageKey, issue.messageParams)}</span>
            </div>
          ))}
          {infoIssues.map((issue, idx) => (
            <div key={idx} className="flex items-center gap-2 text-muted-foreground">
              <span>{t(issue.messageKey, issue.messageParams)}</span>
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
          {errorIssue
            ? t('settings.sections.shortcuts.confirmOverride')
            : t('settings.sections.shortcuts.confirm')}
        </Button>
        <Button size="sm" variant="ghost" onClick={handleCancelClick}>
          {t('settings.sections.shortcuts.cancel')}
        </Button>
      </div>
    </div>
  )
}
