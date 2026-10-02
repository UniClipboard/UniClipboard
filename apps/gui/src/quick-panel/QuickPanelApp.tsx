import { listen } from '@tauri-apps/api/event'
import { LazyMotion, domMax } from 'framer-motion'
import React, { useCallback, useEffect, useEffectEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { daemonClient } from '@/api/daemon/client'
import VisualEffectsProvider from '@/components/motion/VisualEffectsProvider'
import { Button } from '@/components/ui/button'
import { toast } from '@/components/ui/toast'
import { Toaster } from '@/components/ui/toaster'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import { useContentUnlocked } from '@/hooks/useContentUnlocked'
import { usePlatform } from '@/hooks/usePlatform'
import { useThemeSync } from '@/hooks/useThemeSync'
import { useVisualEffectsSampling } from '@/hooks/useVisualEffectsSampling'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { readStoredUiScale } from '@/lib/ui-scale'
import { visualEffectsStore } from '@/lib/visual-effects-store'
import ClipboardHistoryPanel from './ClipboardHistoryPanel'
import QuickPanelScaleIndicator from './components/QuickPanelScaleIndicator'
import { getQuickPanelLayoutClassNames } from './constants'
import { setQuickPanelLayout } from './window-layout'

const log = createLogger('quick-panel-app')
const SHOW_FALLBACK_DELAY_MS = 50

function QuickPanelEffectsSampling({ active }: { active: boolean }) {
  useVisualEffectsSampling(active)
  return null
}

const QuickPanelApp: React.FC = () => {
  const { t } = useTranslation(undefined, { keyPrefix: 'quickPanel' })
  const { isLinux, isTauri } = usePlatform()
  const layoutClassNames = getQuickPanelLayoutClassNames(isLinux && isTauri)
  const [daemonReady, setDaemonReady] = useState(daemonClient.initialized)
  const { unlocked: contentUnlocked } = useContentUnlocked(daemonReady)
  useThemeSync(daemonReady)
  const [bootstrapError, setBootstrapError] = useState<string | null>(null)
  const [showRequestId, setShowRequestId] = useState(0)
  const [preparedRequestId, setPreparedRequestId] = useState(0)
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
      setPreparedRequestId(requestId)
      clearFinalizeTimer()
      void setQuickPanelLayout(readStoredUiScale(), false)
        .then(() => {
          // A newer prepare-show may have arrived during the IPC hop; if so this
          // request is stale and the newer one will finalize itself.
          if (nextShowRequestIdRef.current !== requestId) return
          return visualEffectsStore.refresh().then(() => commands.finalizeQuickPanelShow())
        })
        .catch(err => {
          log.warn({ err }, 'failed to finalize quick panel show')
        })
    },
    [clearFinalizeTimer]
  )
  const finalizeShowFromEffect = useEffectEvent(finalizeShow)

  useEffect(() => {
    const unlistenPrepare = listen('quick-panel://prepare-show', () => {
      const requestId = nextShowRequestIdRef.current + 1
      nextShowRequestIdRef.current = requestId
      pendingShowRequestIdRef.current = requestId
      setShowRequestId(requestId)

      clearFinalizeTimer()
      finalizeTimerRef.current = setTimeout(() => {
        finalizeTimerRef.current = null
        finalizeShowFromEffect(requestId)
      }, SHOW_FALLBACK_DELAY_MS)
    })

    let active = true
    void unlistenPrepare
      .then(() => {
        if (!active) return
        return commands.markQuickPanelReady()
      })
      .catch(err => {
        log.warn({ err }, 'failed to mark quick panel ready')
      })

    return () => {
      active = false
      clearFinalizeTimer()
      unlistenPrepare.then(fn => fn())
    }
  }, [clearFinalizeTimer])

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
      <div
        className={`flex h-screen w-screen items-center justify-center ${layoutClassNames.statusSurface} px-6 text-center text-[13px] text-destructive`}
      >
        {t('unavailable')}
      </div>
    ) : (
      <div
        className={`flex h-screen w-screen items-center justify-center ${layoutClassNames.statusSurface} text-[13px] text-muted-foreground`}
      >
        {t('loading')}
      </div>
    )
  } else if (contentUnlocked !== true) {
    content = (
      <div
        className={`flex h-screen w-screen flex-col items-center justify-center gap-3 ${layoutClassNames.statusSurface} px-6 text-center text-[13px] text-muted-foreground`}
      >
        {t('history.locked.description')}
        <Button
          onClick={() =>
            void commands.showContentUnlock().catch(err => {
              log.warn({ err }, 'Could not open content unlock window')
              toast.error(t('history.locked.openFailed'))
            })
          }
        >
          {t('history.locked.action')}
        </Button>
      </div>
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
  // in this window's tree. Themed via the app's CSS vars (see toaster.tsx), so it
  // follows the panel's light/dark class without a next-themes provider.
  return (
    <LazyMotion features={domMax} strict>
      <VisualEffectsProvider>
        <QuickPanelEffectsSampling
          active={daemonReady && showRequestId > 0 && preparedRequestId === showRequestId}
        />
        {content}
        {isLinux && isTauri && daemonReady && (
          <ShortcutProvider key={showRequestId}>
            <QuickPanelScaleIndicator />
          </ShortcutProvider>
        )}
        <Toaster />
      </VisualEffectsProvider>
    </LazyMotion>
  )
}

export default QuickPanelApp
