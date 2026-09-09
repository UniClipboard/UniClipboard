import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { installWindowResizeShortcuts, type QuickPanelScaleFeedback } from '../window-layout'

export default function QuickPanelScaleIndicator() {
  const { t } = useTranslation(undefined, { keyPrefix: 'quickPanel' })
  const [feedback, setFeedback] = useState<QuickPanelScaleFeedback | null>(null)

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined
    const dispose = installWindowResizeShortcuts(next => {
      setFeedback(next)
      clearTimeout(timer)
      timer = setTimeout(() => setFeedback(null), 1200)
    })
    return () => {
      dispose()
      clearTimeout(timer)
    }
  }, [])

  if (!feedback) return null

  return (
    <div className="pointer-events-none fixed bottom-3 right-3 z-[1000]">
      <div
        role="status"
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
      </div>
    </div>
  )
}
