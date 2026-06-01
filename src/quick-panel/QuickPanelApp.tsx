import { LazyMotion, MotionConfig, domMax } from 'framer-motion'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { daemonClient } from '@/api/daemon/client'
import { usePlatform } from '@/hooks/usePlatform'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import ClipboardHistoryPanel from './ClipboardHistoryPanel'

const loadingClassName =
  'flex h-screen w-screen items-center justify-center bg-transparent text-[13px] text-muted-foreground'

const errorClassName =
  'flex h-screen w-screen items-center justify-center bg-transparent px-6 text-center text-[13px] text-destructive'

const QuickPanelApp: React.FC = () => {
  const { t } = useTranslation(undefined, { keyPrefix: 'quickPanel' })
  const { reduceVisualEffects } = usePlatform()
  const [daemonReady, setDaemonReady] = useState(daemonClient.initialized)
  const [bootstrapError, setBootstrapError] = useState<string | null>(null)

  useEffect(() => {
    if (daemonClient.initialized) {
      return
    }

    let cancelled = false

    connectDaemonWs()
      .then(() => {
        if (cancelled) return
        setDaemonReady(true)
        setBootstrapError(null)
      })
      .catch(error => {
        if (cancelled) return
        const message = error instanceof Error ? error.message : String(error)
        setBootstrapError(message)
      })

    return () => {
      cancelled = true
    }
  }, [])

  let content: React.ReactNode
  if (!daemonReady) {
    content = bootstrapError ? (
      <div className={errorClassName}>{t('unavailable')}</div>
    ) : (
      <div className={loadingClassName}>{t('loading')}</div>
    )
  } else {
    content = <ClipboardHistoryPanel />
  }

  // The quick panel is a separate webview from the main window, so it needs its
  // own framer-motion provider — without it, `m.*` elements stay stuck at their
  // `initial` state (e.g. opacity 0) and never render. Mirrors src/App.tsx.
  return (
    <LazyMotion features={domMax} strict>
      <MotionConfig reducedMotion={reduceVisualEffects ? 'always' : 'user'}>{content}</MotionConfig>
    </LazyMotion>
  )
}

export default QuickPanelApp
