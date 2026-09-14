import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuickPanelScaleShortcuts } from '../hooks/useQuickPanelScaleShortcuts'
import { type QuickPanelScaleFeedback } from '../window-layout'

export default function QuickPanelScaleIndicator() {
  const { t } = useTranslation(undefined, { keyPrefix: 'quickPanel' })
  const [feedback, setFeedback] = useState<QuickPanelScaleFeedback | null>(null)

  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  useQuickPanelScaleShortcuts(next => {
    setFeedback(next)
    clearTimeout(timer.current)
    timer.current = setTimeout(() => setFeedback(null), 1200)
  })
  useEffect(() => () => clearTimeout(timer.current), [])

  if (!feedback) return null

  return (
    <div className="pointer-events-none fixed bottom-3 right-3 z-[1000]">
      <output
        aria-live="polite"
        aria-atomic="true"
        className="flex items-center gap-4 rounded-xl border border-border/60 bg-card/95 px-4 py-3 text-[13px] text-card-foreground shadow-lg"
      >
        <span className="flex items-center gap-2">
          <span className="text-muted-foreground">{t('scaleText')}</span>
          <span className="font-medium tabular-nums">{feedback.textPercent}%</span>
        </span>
        <span className="h-4 w-px bg-border/60" aria-hidden="true" />
        <span className="flex items-center gap-2">
          <span className="text-muted-foreground">{t('scaleWindow')}</span>
          <span className="font-medium tabular-nums">{feedback.windowPercent}%</span>
        </span>
      </output>
    </div>
  )
}
