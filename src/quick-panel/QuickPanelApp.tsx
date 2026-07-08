import { listen } from '@tauri-apps/api/event'
import { LazyMotion, MotionConfig, domMax } from 'framer-motion'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { daemonClient } from '@/api/daemon/client'
import { Toaster } from '@/components/ui/sonner'
import { usePlatform } from '@/hooks/usePlatform'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { readStoredUiScale } from '@/lib/ui-scale'
import ClipboardHistoryPanel from './ClipboardHistoryPanel'

const log = createLogger('quick-panel-app')
const SHOW_FALLBACK_DELAY_MS = 50

const loadingClassName =
  'flex h-screen w-screen items-center justify-center bg-transparent text-[13px] text-muted-foreground'

const errorClassName =
  'flex h-screen w-screen items-center justify-center bg-transparent px-6 text-center text-[13px] text-destructive'

const QuickPanelApp: React.FC = () => {
  const { t } = useTranslation(undefined, { keyPrefix: 'quickPanel' })
  const { reduceVisualEffects } = usePlatform()
  const [daemonReady, setDaemonReady] = useState(daemonClient.initialized)
  const [bootstrapError, setBootstrapError] = useState<string | null>(null)
  const [showRequestId, setShowRequestId] = useState(0)
  const nextShowRequestIdRef = useRef(0)
  const pendingShowRequestIdRef = useRef<number | null>(null)
  const finalizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const clearFinalizeTimer = useCallback(() => {
    if (finalizeTimerRef.current !== null) {
      clearTimeout(finalizeTimerRef.current)
      finalizeTimerRef.current = null
    }
  }, [])

  const finalizeShow = useCallback(
    (requestId: number) => {
      if (pendingShowRequestIdRef.current !== requestId) return
      pendingShowRequestIdRef.current = null
      clearFinalizeTimer()
      void commands
        .setQuickPanelLayout(readStoredUiScale(), false)
        .then(() => {
          // A newer prepare-show may have arrived during the IPC hop; if so this
          // request is stale and the newer one will finalize itself.
          if (nextShowRequestIdRef.current !== requestId) return
          return commands.finalizeQuickPanelShow()
        })
        .catch(err => {
          log.warn({ err }, 'failed to finalize quick panel show')
        })
    },
    [clearFinalizeTimer]
  )

  useEffect(() => {
    const unlistenPrepare = listen('quick-panel://prepare-show', () => {
      const requestId = nextShowRequestIdRef.current + 1
      nextShowRequestIdRef.current = requestId
      pendingShowRequestIdRef.current = requestId
      setShowRequestId(requestId)

      clearFinalizeTimer()
      finalizeTimerRef.current = setTimeout(() => {
        finalizeTimerRef.current = null
        finalizeShow(requestId)
      }, SHOW_FALLBACK_DELAY_MS)
    })

    return () => {
      clearFinalizeTimer()
      unlistenPrepare.then(fn => fn())
    }
  }, [clearFinalizeTimer, finalizeShow])

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
    content = <ClipboardHistoryPanel showRequestId={showRequestId} onShowPrepared={finalizeShow} />
  }

  // The quick panel is a separate webview from the main window, so it needs its
  // own framer-motion provider — without it, `m.*` elements stay stuck at their
  // `initial` state (e.g. opacity 0) and never render. Mirrors src/App.tsx.
  //
  // The Toaster is likewise per-webview: the reused history context menu surfaces
  // send/reveal feedback through `toast`, which no-ops without a Toaster mounted
  // in this window's tree. Themed via the app's CSS vars (see sonner.tsx), so it
  // follows the panel's light/dark class without a next-themes provider.
  return (
    <LazyMotion features={domMax} strict>
      <MotionConfig reducedMotion={reduceVisualEffects ? 'always' : 'user'}>{content}</MotionConfig>
      <Toaster />
    </LazyMotion>
  )
}

export default QuickPanelApp
