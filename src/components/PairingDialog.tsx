import {
  Loader2,
  RefreshCw,
  Monitor,
  Smartphone,
  Laptop,
  AlertCircle,
  ShieldCheck,
} from 'lucide-react'
import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { classifyPairingError } from '@/api/daemon/events'
import { getP2PPeers, initiateP2PPairing, verifyP2PPairingPin } from '@/api/daemon/pairing'
import type { P2PPeerInfo } from '@/api/daemon/pairing'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog'
import { toast } from '@/components/ui/toast'
import { usePairingEvents } from '@/hooks/useDaemonEvents'
import { createLogger } from '@/lib/logger'
import { formatPeerIdForDisplay } from '@/lib/utils'

const log = createLogger('pairing-dialog')
// import { getLocalDeviceName } from '@/api/deviceConnection' // Assuming we'll add this or use existing

// Mock getLocalDeviceName if not available yet, or import if available
// For now, let's assume we might need to fetch it or just display peers.

type PairingStep = 'discovery' | 'connecting' | 'pin-verify' | 'success' | 'failed'

interface PairingDialogProps {
  open: boolean
  onClose: () => void
  onPairingSuccess?: () => void
}

export default function PairingDialog({ open, onClose, onPairingSuccess }: PairingDialogProps) {
  const { t } = useTranslation()
  const [step, setStep] = useState<PairingStep>('discovery')
  const [peers, setPeers] = useState<P2PPeerInfo[]>([])
  const [loading, setLoading] = useState(false)
  const [selectedPeer, setSelectedPeer] = useState<P2PPeerInfo | null>(null)
  const [pairingSessionId, setPairingSessionId] = useState<string | null>(null)
  const [pinCode, setPinCode] = useState<string>('')
  const [errorMsg, setErrorMsg] = useState<string>('')
  const [isPinVerifying, setIsPinVerifying] = useState(false)
  const pairingSessionIdRef = React.useRef<string | null>(null)

  const localizePairingError = React.useCallback(
    (error?: string | null) => {
      switch (classifyPairingError(error)) {
        case 'active_session_exists':
          return t('pairing.failed.errors.activeSession')
        case 'no_local_participant':
          return t('pairing.failed.errors.noParticipant')
        case 'session_not_found':
          return t('pairing.failed.errors.sessionExpired')
        case 'daemon_unavailable':
          return t('pairing.failed.errors.daemonUnavailable')
        default:
          return error || t('pairing.failed.errors.initiate')
      }
    },
    [t]
  )

  useEffect(() => {
    pairingSessionIdRef.current = pairingSessionId
  }, [pairingSessionId])

  // Use usePairingEvents for subscription (replaces onP2PPairingVerification)
  usePairingEvents({
    onVerification: ({ sessionId, deviceName, code }) => {
      const currentSessionId = pairingSessionIdRef.current
      if (currentSessionId && sessionId !== currentSessionId) {
        return
      }

      log.debug({ sessionId, deviceName }, 'Verification event')
      pairingSessionIdRef.current = sessionId
      setPairingSessionId(sessionId)
      setPinCode(code ?? '')
      setStep('pin-verify')
      setIsPinVerifying(false)
    },

    onVerifying: () => {
      setIsPinVerifying(true)
    },

    onComplete: ({ sessionId, deviceName }) => {
      if (pairingSessionIdRef.current && sessionId !== pairingSessionIdRef.current) {
        return
      }
      log.debug({ sessionId, deviceName }, 'Complete event')
      pairingSessionIdRef.current = null
      setStep('success')
      setIsPinVerifying(false)
      toast.success(t('pairing.success.title'))
      setTimeout(() => {
        onPairingSuccess?.()
        onClose()
      }, 2000)
    },

    onFailed: ({ sessionId, error }) => {
      if (pairingSessionIdRef.current && sessionId !== pairingSessionIdRef.current) {
        return
      }
      log.error({ sessionId, error }, 'Failed event')
      const message = localizePairingError(error)
      pairingSessionIdRef.current = null
      setErrorMsg(message)
      setStep('failed')
      setIsPinVerifying(false)
      toast.error(t('pairing.failed.title'), {
        description: message,
      })
    },
  })

  const loadPeers = React.useCallback(async () => {
    setLoading(true)
    setErrorMsg('')
    try {
      const list = await getP2PPeers()
      // Filter out already paired devices if needed, or just show status
      // For now show all discovered
      setPeers(list)
    } catch (err) {
      log.error({ err }, 'Failed to load peers')
      setErrorMsg(t('pairing.failed.errors.loadPeers'))
    } finally {
      setLoading(false)
    }
  }, [t])

  useEffect(() => {
    if (open) {
      log.debug('Dialog opened, initializing')

      // Reset state
      setStep('discovery')
      setPeers([])
      setSelectedPeer(null)
      setPairingSessionId(null)
      pairingSessionIdRef.current = null
      setPinCode('')
      setErrorMsg('')
      setIsPinVerifying(false)

      // Load peers
      void loadPeers()
    } else {
      // Cleanup when closed
      log.debug('Dialog closed')
      pairingSessionIdRef.current = null
    }
  }, [open, loadPeers])

  const handleConnect = async (peer: P2PPeerInfo) => {
    setSelectedPeer(peer)
    setStep('connecting')
    setErrorMsg('')
    try {
      const response = await initiateP2PPairing({ peerId: peer.peerId })
      if (response.success) {
        pairingSessionIdRef.current = response.sessionId
        setPairingSessionId(response.sessionId)
        // Wait for PIN ready event...
      } else {
        setErrorMsg(localizePairingError(response.error))
        setStep('failed')
      }
    } catch (err) {
      log.error({ err }, 'Failed to initiate pairing')
      setErrorMsg(localizePairingError(err instanceof Error ? err.message : String(err)))
      setStep('failed')
    }
  }

  const handlePinConfirm = async (matches: boolean) => {
    if (!pairingSessionId) return
    setIsPinVerifying(true)
    try {
      await verifyP2PPairingPin(pairingSessionId, matches)
      if (!matches) {
        setIsPinVerifying(false)
        pairingSessionIdRef.current = null
        onClose() // User rejected
      }
      // If matches, wait for 'success' or 'failed' event
    } catch (err) {
      log.error({ err }, 'Failed to verify PIN')
      setErrorMsg(t('pairing.failed.errors.verifyPin'))
      setStep('failed')
      setIsPinVerifying(false)
    }
  }

  const getDeviceIcon = (name?: string | null) => {
    const n = (name || '').toLowerCase()
    if (n.includes('phone') || n.includes('iphone') || n.includes('android'))
      return <Smartphone className="w-5 h-5" />
    if (n.includes('mac') || n.includes('book') || n.includes('laptop'))
      return <Laptop className="w-5 h-5" />
    return <Monitor className="w-5 h-5" />
  }

  return (
    <Dialog open={open} onOpenChange={open => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {step === 'discovery' && t('pairing.steps.discovery')}
            {step === 'connecting' && t('pairing.steps.connecting')}
            {step === 'pin-verify' && t('pairing.steps.pinVerify')}
            {step === 'success' && t('pairing.steps.success')}
            {step === 'failed' && t('pairing.steps.failed')}
          </DialogTitle>
          <DialogDescription>
            {step === 'discovery' && t('pairing.discovery.description')}
            {step === 'pin-verify' && t('pairing.pinVerify.description')}
            {step === 'failed' && errorMsg}
          </DialogDescription>
        </DialogHeader>

        <div className="py-6 min-h-[200px] flex flex-col">
          {step === 'discovery' && (
            <div className="flex-1 space-y-4">
              {loading && peers.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-40 space-y-4 text-muted-foreground">
                  <Loader2 className="w-8 h-8 animate-spin" />
                  <p>{t('pairing.discovery.searching')}</p>
                </div>
              ) : peers.length === 0 ? (
                <div className="flex flex-col items-center justify-center h-40 space-y-4 text-muted-foreground">
                  <Monitor className="w-12 h-12 opacity-20" />
                  <p>{t('pairing.discovery.noDevices')}</p>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={loadPeers}
                    disabled={loading}
                    className="gap-2"
                  >
                    <RefreshCw className={`w-3 h-3 ${loading ? 'animate-spin' : ''}`} />
                    {t('pairing.discovery.reScan')}
                  </Button>
                </div>
              ) : (
                <div className="space-y-2">
                  <div className="flex items-center justify-between text-xs text-muted-foreground px-1 pb-2">
                    <span>{t('pairing.discovery.foundCount', { count: peers.length })}</span>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6"
                      onClick={loadPeers}
                      disabled={loading}
                    >
                      <RefreshCw className={`w-3 h-3 ${loading ? 'animate-spin' : ''}`} />
                    </Button>
                  </div>
                  <div className="space-y-2 overflow-y-auto max-h-[300px]">
                    {peers.map(peer => (
                      <button
                        key={peer.peerId}
                        type="button"
                        className="flex items-center justify-between p-3 rounded-lg border bg-card hover:bg-accent/50 transition-colors cursor-pointer group"
                        onClick={() => handleConnect(peer)}
                        onKeyDown={event => {
                          if (event.key === 'Enter' || event.key === ' ') {
                            event.preventDefault()
                            handleConnect(peer)
                          }
                        }}
                      >
                        <div className="flex items-center gap-3">
                          <div className="p-2 bg-primary/10 rounded-full text-primary">
                            {getDeviceIcon(peer.deviceName)}
                          </div>
                          <div>
                            <h4 className="font-medium text-sm">
                              {peer.deviceName || t('pairing.discovery.unknownDevice')}
                            </h4>
                            <p className="text-xs text-muted-foreground truncate w-40">
                              ID: {formatPeerIdForDisplay(peer.peerId)}
                            </p>
                          </div>
                        </div>
                        <Button
                          size="sm"
                          variant="secondary"
                          className="opacity-0 group-hover:opacity-100 transition-opacity"
                        >
                          {t('pairing.discovery.connect')}
                        </Button>
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}

          {step === 'connecting' && (
            <div className="flex flex-col items-center justify-center h-48 space-y-4">
              <div className="relative">
                <div className="w-16 h-16 rounded-full bg-primary/10 flex items-center justify-center animate-pulse">
                  <Monitor className="w-8 h-8 text-primary" />
                </div>
                <Loader2 className="absolute -bottom-2 -right-2 w-6 h-6 animate-spin text-primary" />
              </div>
              <div className="text-center space-y-1">
                <h3 className="font-medium">
                  {t('pairing.connecting.title', { deviceName: selectedPeer?.deviceName })}
                </h3>
                <p className="text-sm text-muted-foreground">
                  {t('pairing.connecting.pleaseWait')}
                </p>
              </div>
            </div>
          )}

          {step === 'pin-verify' && (
            <div className="flex flex-col items-center justify-center space-y-6 py-4">
              <div className="p-4 bg-muted/50 rounded-xl border-2 border-dashed border-primary/20 text-center w-full max-w-[240px]">
                <p className="text-sm text-muted-foreground mb-2">
                  {t('pairing.pinVerify.pinLabel')}
                </p>
                <div className="text-4xl font-mono font-bold tracking-wider text-primary">
                  {pinCode}
                </div>
              </div>

              <div className="flex items-center gap-2 text-sm text-muted-foreground bg-amber-500/10 text-amber-600 px-4 py-2 rounded-full">
                <ShieldCheck className="w-4 h-4" />
                {t('pairing.pinVerify.warning')}
              </div>

              <div className="flex gap-4 w-full">
                <Button
                  variant="outline"
                  className="flex-1"
                  onClick={() => handlePinConfirm(false)}
                  disabled={isPinVerifying}
                >
                  {t('pairing.pinVerify.notMatch')}
                </Button>
                <Button
                  className="flex-1"
                  onClick={() => handlePinConfirm(true)}
                  disabled={isPinVerifying}
                >
                  {isPinVerifying ? (
                    <span className="flex items-center gap-2">
                      <Loader2 className="w-4 h-4 animate-spin" />
                      {t('pairing.pinVerify.verifying')}
                    </span>
                  ) : (
                    t('pairing.pinVerify.match')
                  )}
                </Button>
              </div>
            </div>
          )}

          {step === 'success' && (
            <div className="flex flex-col items-center justify-center h-48 space-y-4 text-green-600">
              <div className="w-16 h-16 rounded-full bg-green-100 flex items-center justify-center">
                <ShieldCheck className="w-8 h-8" />
              </div>
              <h3 className="text-lg font-medium">{t('pairing.success.title')}</h3>
            </div>
          )}

          {step === 'failed' && (
            <div className="flex flex-col items-center justify-center space-y-6 py-4">
              <div className="w-16 h-16 rounded-full bg-destructive/10 flex items-center justify-center text-destructive">
                <AlertCircle className="w-8 h-8" />
              </div>
              <div className="text-center space-y-2">
                <h3 className="font-medium text-destructive">{t('pairing.failed.title')}</h3>
                <p className="text-sm text-muted-foreground">{errorMsg}</p>
              </div>
              <Button
                onClick={() => {
                  pairingSessionIdRef.current = null
                  setPairingSessionId(null)
                  setStep('discovery')
                }}
                className="w-full"
              >
                {t('pairing.failed.retry')}
              </Button>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
