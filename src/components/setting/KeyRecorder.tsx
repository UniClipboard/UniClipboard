'use client'

import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui'
import { formatShortcutChord } from '@/lib/shortcut-format'
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
  // Committed chord segments (one combo each), at most MAX_CHORD_SEGMENTS.
  const [segments, setSegments] = useState<string[]>([])

  // Hold the latest onCancel in a ref so the key-capture effect stays mount-only.
  const onCancelRef = useRef(onCancel)
  useEffect(() => {
    onCancelRef.current = onCancel
  }, [onCancel])

  // Capture key presses globally while recording. Each non-modifier keydown
  // appends one segment (VS Code style); Escape cancels.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      // Ignore auto-repeat events from a held key; only the initial press
      // should record a segment (otherwise holding a key fabricates a second
      // identical segment, i.e. an accidental double-tap chord).
      if (e.repeat) {
        e.preventDefault()
        e.stopPropagation()
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        onCancelRef.current()
        return
      }
      e.preventDefault()
      e.stopPropagation()
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

  const handleClear = () => setSegments([])

  const handleCancelClick = () => {
    onCancel()
  }

  const chordSegments = formatShortcutChord(candidateKey)
  const isFull = segments.length >= MAX_CHORD_SEGMENTS

  return (
    <div className="flex flex-col gap-2 p-3 rounded-md border-2 border-primary/50 bg-card">
      <div className="flex items-center gap-2 min-h-7">
        {chordSegments.length > 0 ? (
          <div className="flex items-center gap-1.5">
            {chordSegments.map((parts, segIdx) => (
              <div key={`seg-${segIdx}`} className="flex items-center gap-1.5">
                {segIdx > 0 && <span className="text-muted-foreground text-xs">›</span>}
                <div className="flex items-center gap-0.5">
                  {parts.map((part, idx) => (
                    <span key={`${part}-${idx}`} className="flex items-center">
                      {idx > 0 && <span className="text-muted-foreground text-xs mx-0.5">+</span>}
                      <kbd className="bg-muted text-xs font-mono px-1.5 py-0.5 rounded border border-border/60 text-foreground">
                        {part}
                      </kbd>
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        ) : (
          <span className="text-sm text-muted-foreground">
            {t('settings.sections.shortcuts.recording')}
          </span>
        )}
      </div>

      {/* Hint: after one segment, the user may add a second to form a chord. */}
      {segments.length > 0 && !isFull && (
        <span className="text-xs text-muted-foreground">
          {t('settings.sections.shortcuts.chordHint')}
        </span>
      )}

      {/* Conflict warnings */}
      {issues.length > 0 && (
        <div className="flex flex-col gap-1 text-xs">
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

      {/* Action buttons */}
      <div className="flex items-center gap-2 mt-1">
        <Button
          size="sm"
          variant={errorIssue ? 'default' : 'outline'}
          onClick={handleConfirm}
          disabled={!candidateKey}
        >
          {errorIssue
            ? t('settings.sections.shortcuts.confirmOverride')
            : t('settings.sections.shortcuts.confirm')}
        </Button>
        {segments.length > 0 && (
          <Button size="sm" variant="ghost" onClick={handleClear}>
            {t('settings.sections.shortcuts.rerecord')}
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={handleCancelClick}>
          {t('settings.sections.shortcuts.cancel')}
        </Button>
      </div>
    </div>
  )
}
