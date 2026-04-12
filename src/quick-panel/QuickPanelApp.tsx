import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import ClipboardHistoryPanel from './ClipboardHistoryPanel'
import { daemonClient } from '@/api/daemon/client'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'

const loadingClassName =
  'flex h-screen w-screen items-center justify-center bg-transparent text-[13px] text-muted-foreground'

const errorClassName =
  'flex h-screen w-screen items-center justify-center bg-transparent px-6 text-center text-[13px] text-destructive'

const QuickPanelApp: React.FC = () => {
  const { t } = useTranslation(undefined, { keyPrefix: 'quickPanel' })
  const [daemonReady, setDaemonReady] = useState(daemonClient.initialized)
  const [bootstrapError, setBootstrapError] = useState<string | null>(null)

  useEffect(() => {
    if (daemonClient.initialized) {
      setDaemonReady(true)
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

  if (!daemonReady) {
    if (bootstrapError) {
      return <div className={errorClassName}>{t('unavailable')}</div>
    }

    return <div className={loadingClassName}>{t('loading')}</div>
  }

  return <ClipboardHistoryPanel />
}

export default QuickPanelApp
