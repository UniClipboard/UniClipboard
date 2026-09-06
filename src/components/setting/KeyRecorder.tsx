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
import { MAX_CHORD_SEGMENTS, normalizeHotkey } from '@/shortcuts/normalize'

interface KeyRecorderProps {
  shortcutId: string
  scope: ShortcutScope
  currentOverrides: ShortcutKeyOverrides
  onConfirm: (key: string, clearedIds?: string[]) => void
  onCancel: () => void
}

/** Physical modifier keys, which alone never form a chord segment. */
const MODIFIER_KEYS = new Set(['Control', 'Shift', 'Alt', 'Meta'])

/**
 * Build a normalized single combo from a keydown event, or `null` if the event
 * is a bare modifier press (we wait for a real key). Modifier state is read off
 * the event so holding Cmd and tapping V twice yields two `meta+v` segments.
 */
function comboFromEvent(e: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null
  const parts: string[] = []
  if (e.ctrlKey) parts.push('ctrl')
  if (e.altKey) parts.push('alt')
  if (e.shiftKey) parts.push('shift')
  if (e.metaKey) parts.push('meta')
  parts.push(e.key.toLowerCase())
  return normalizeHotkey(parts.join('+'))
}

export function KeyRecorder({
  shortcutId,
  scope,
  currentOverrides,
  onConfirm,
  onCancel,
}: KeyRecorderProps) {
  const { t } = useTranslation()
  const recordingRef = useRef<HTMLDivElement>(null)
  useShortcutLayer({ layer: 'modal', scope: 'modal' })
  // Committed chord segments (one combo each), at most MAX_CHORD_SEGMENTS.
  const [segments, setSegments] = useState<string[]>([])

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
      const combo = comboFromEvent(e)
      if (!combo) return
      setSegments(prev => (prev.length >= MAX_CHORD_SEGMENTS ? prev : [...prev, combo]))
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
    if (!candidateKey) return
    onConfirm(candidateKey, errorIssue?.relatedIds)
  }

  const handleClear = () => {
    setSegments([])
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
            <span className="text-sm text-muted-foreground">
              {t('settings.sections.shortcuts.recording')}
            </span>
          )}
        </span>
      </div>

      {/* Hint: after one segment, the user may add a second to form a chord. */}
      {segments.length > 0 && !isFull && (
        <span className="text-xs text-muted-foreground">
          {t('settings.sections.shortcuts.chordHint')}
        </span>
      )}

      {/* Conflict warnings */}
      {issues.length > 0 && (
        <div
          role="alert"
          className="flex flex-col gap-2 rounded-lg border border-border bg-muted/30 p-2.5 text-xs leading-relaxed"
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
          disabled={!candidateKey}
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
