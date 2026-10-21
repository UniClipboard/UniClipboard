'use client'

import { useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ShortcutKeys } from '@/components/setting/ShortcutKeys'
import { Button } from '@/components/ui'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import {
  getCandidateKeyIssues,
  resolveShortcuts,
  type ShortcutKeyOverrides,
} from '@/shortcuts/conflicts'
import { SHORTCUT_DEFINITIONS, type ShortcutScope } from '@/shortcuts/definitions'
import { MAX_CHORD_SEGMENTS } from '@/shortcuts/normalize'
import { comboFromEvent } from '@/shortcuts/record'

interface KeyRecorderProps {
  shortcutId: string
  scope: ShortcutScope
  currentOverrides: ShortcutKeyOverrides
  onConfirm: (key: string, clearedIds?: string[]) => void
  onCancel: () => void
  /** The last save failed; the recorder stays open and shows why. */
  saveError?: string | null
  saving?: boolean
}

export function KeyRecorder({
  shortcutId,
  scope,
  currentOverrides,
  onConfirm,
  onCancel,
  saveError,
  saving = false,
}: KeyRecorderProps) {
  const { t } = useTranslation()
  const recordingRef = useRef<HTMLDivElement>(null)
  useShortcutLayer({ layer: 'modal', scope: 'modal' })
  // Committed chord segments (one combo each), at most MAX_CHORD_SEGMENTS.
  const [segments, setSegments] = useState<string[]>([])
  // The last key pressed has no name in the stored token set; it was not recorded.
  const [unsupportedKey, setUnsupportedKey] = useState<string | null>(null)

  const cancelRecording = useEffectEvent(onCancel)

  // Capture before application handlers while the recording field has focus.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        cancelRecording()
        return
      }
      // Only the focused recording field captures combinations; actions remain keyboard accessible.
      if (e.target !== recordingRef.current) return
      if (e.key === 'Tab' && !e.metaKey && !e.ctrlKey && !e.altKey) return
      e.preventDefault()
      e.stopPropagation()
      if (e.repeat) return
      const recorded = comboFromEvent(e)
      if (!recorded) return
      if ('unsupported' in recorded) {
        setUnsupportedKey(recorded.unsupported)
        return
      }
      setUnsupportedKey(null)
      setSegments(prev => (prev.length >= MAX_CHORD_SEGMENTS ? prev : [...prev, recorded.combo]))
    }
    window.addEventListener('keydown', onKeyDown, true)
    return () => window.removeEventListener('keydown', onKeyDown, true)
  }, [])

  // The candidate value is the space-joined chord sequence.
  const candidateKey = segments.join(' ')

  const resolvedShortcuts = useMemo(
    () => resolveShortcuts(SHORTCUT_DEFINITIONS, currentOverrides),
    [currentOverrides]
  )

  const issues = useMemo(() => {
    if (!candidateKey) return []
    return getCandidateKeyIssues(resolvedShortcuts, {
      id: shortcutId,
      scope,
      key: candidateKey,
    })
  }, [candidateKey, resolvedShortcuts, shortcutId, scope])

  const errorIssue = issues.find(i => i.level === 'error')
  const warningIssues = issues.filter(i => i.level === 'warning')
  const infoIssues = issues.filter(i => i.level === 'info')

  const handleConfirm = () => {
    if (!candidateKey || saving) return
    onConfirm(candidateKey, errorIssue?.relatedIds)
  }

  const handleClear = () => {
    setSegments([])
    setUnsupportedKey(null)
    recordingRef.current?.focus()
  }

  const handleCancelClick = () => {
    onCancel()
  }

  const isFull = segments.length >= MAX_CHORD_SEGMENTS

  return (
    <div className="flex flex-col gap-3">
      <div
        ref={recordingRef}
        role="group"
        tabIndex={0}
        aria-label={t('settings.sections.shortcuts.recording')}
        className="flex min-h-14 items-center justify-center rounded-lg border border-border bg-muted/30 p-3 text-center outline-none focus:border-ring focus:ring-2 focus:ring-ring/20"
      >
        <span aria-live="polite">
          {candidateKey ? (
            <ShortcutKeys shortcut={candidateKey} />
          ) : (
            <span className="text-ui-body text-muted-foreground">
              {t('settings.sections.shortcuts.recording')}
            </span>
          )}
        </span>
      </div>

      {/* Hint: after one segment, the user may add a second to form a chord. */}
      {segments.length > 0 && !isFull && (
        <span className="text-ui-caption text-muted-foreground">
          {t('settings.sections.shortcuts.chordHint')}
        </span>
      )}

      {(unsupportedKey || saveError) && (
        <div role="alert" className="flex flex-col gap-1 text-ui-caption-relaxed text-destructive">
          {unsupportedKey && (
            <span>{t('settings.sections.shortcuts.unsupportedKey', { key: unsupportedKey })}</span>
          )}
          {saveError && (
            <span>{t('settings.sections.shortcuts.saveFailed', { message: saveError })}</span>
          )}
        </div>
      )}

      {/* Conflict warnings */}
      {issues.length > 0 && (
        <div
          role="alert"
          className="flex flex-col gap-2 rounded-lg border border-border bg-muted/30 p-2.5 text-ui-caption-relaxed"
        >
          {errorIssue && (
            <div className="flex items-center gap-2 text-destructive">
              <span>{t(errorIssue.messageKey, errorIssue.messageParams)}</span>
            </div>
          )}
          {warningIssues.map(issue => (
            <div
              key={`warning-${issue.messageKey}`}
              className="flex items-center gap-2 text-yellow-600 dark:text-yellow-400"
            >
              <span>{t(issue.messageKey, issue.messageParams)}</span>
            </div>
          ))}
          {infoIssues.map(issue => (
            <div
              key={`info-${issue.messageKey}`}
              className="flex items-center gap-2 text-muted-foreground"
            >
              <span>{t(issue.messageKey, issue.messageParams)}</span>
            </div>
          ))}
        </div>
      )}

      <div className="flex flex-wrap items-center justify-end gap-2 border-t border-border/50 pt-3">
        {segments.length > 0 && (
          <Button size="sm" variant="ghost" className="mr-auto" onClick={handleClear}>
            {t('settings.sections.shortcuts.rerecord')}
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={handleCancelClick}>
          {t('settings.sections.shortcuts.cancel')}
        </Button>
        <Button
          size="sm"
          onClick={handleConfirm}
          disabled={!candidateKey || saving}
          className="h-auto min-h-7 whitespace-normal"
        >
          {errorIssue
            ? t('settings.sections.shortcuts.confirmOverride')
            : t('settings.sections.shortcuts.save')}
        </Button>
      </div>
    </div>
  )
}
