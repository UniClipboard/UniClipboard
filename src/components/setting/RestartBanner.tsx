import { Check, RefreshCw, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

export interface RestartBannerProps {
  visible: boolean
  message: string
  onRestart: () => Promise<void>
  loading?: boolean
  error?: string | null
  onDismissError?: () => void
}

const SUCCESS_DISPLAY_MS = 1500

export function RestartBanner({
  visible,
  message,
  onRestart,
  loading = false,
  error = null,
  onDismissError,
}: RestartBannerProps) {
  const { t } = useTranslation()
  const [showSuccess, setShowSuccess] = useState(false)
  const prevVisibleRef = useRef(visible)

  useEffect(() => {
    if (prevVisibleRef.current && !visible && !error) {
      setShowSuccess(true)
      const timer = window.setTimeout(() => setShowSuccess(false), SUCCESS_DISPLAY_MS)
      return () => window.clearTimeout(timer)
    }
    prevVisibleRef.current = visible
  }, [visible, error])

  if (showSuccess) {
    return (
      <output
        aria-live="polite"
        className="flex items-center gap-2 px-4 py-3 bg-emerald-500/10 border-b border-emerald-500/20 rounded-none"
      >
        <Check
          className="size-4 text-emerald-600 dark:text-emerald-400 shrink-0"
          aria-hidden="true"
        />
        <p className="text-sm text-emerald-700 dark:text-emerald-300">
          {t('settings.restartBanner.successMessage')}
        </p>
      </output>
    )
  }

  if (!visible) return null

  return (
    <output
      aria-live="polite"
      className="flex items-start gap-2 px-4 py-3 bg-accent/40 border-b border-border/40 rounded-none"
    >
      <RefreshCw
        className={`size-4 text-foreground mt-0.5 shrink-0 ${loading ? 'animate-spin' : ''}`}
        aria-hidden="true"
      />
      <div className="flex-1 space-y-1">
        <p className="text-sm text-foreground">{message}</p>
        {error && (
          <p role="alert" className="text-xs text-destructive">
            {error}
          </p>
        )}
      </div>
      <div className="ml-auto flex items-center gap-2">
        {!error ? (
          <Button
            variant="default"
            size="sm"
            onClick={() => {
              void onRestart()
            }}
            disabled={loading}
          >
            {loading
              ? t('settings.restartBanner.restartingButton')
              : t('settings.restartBanner.restartButton')}
          </Button>
        ) : (
          <>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                void onRestart()
              }}
              disabled={loading}
            >
              {t('settings.restartBanner.retryButton')}
            </Button>
            {onDismissError && (
              <Button
                variant="ghost"
                size="icon"
                aria-label={t('settings.restartBanner.dismissAriaLabel')}
                onClick={onDismissError}
              >
                <X className="size-3.5" aria-hidden="true" />
              </Button>
            )}
          </>
        )}
      </div>
    </output>
  )
}
