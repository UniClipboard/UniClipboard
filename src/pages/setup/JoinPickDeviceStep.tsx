import { AnimatePresence, motion } from 'framer-motion'
import { Laptop, Monitor, Radar, RefreshCw, Smartphone } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { JoinPickDeviceStepProps } from './types'
import { Button } from '@/components/ui/button'
import { formatPeerIdForDisplay } from '@/lib/utils'
import StepLayout from '@/pages/setup/StepLayout'

export default function JoinPickDeviceStep({
  onSelectPeer,
  onRescan,
  peers,
  scanPhase,
  error,
  loading,
  direction,
}: JoinPickDeviceStepProps) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.joinPickDevice' })
  const { t: tCommon } = useTranslation(undefined, { keyPrefix: 'setup.common' })

  const resolveErrorMessage = () => {
    if (!error) {
      return null
    }

    switch (error) {
      case 'NetworkTimeout':
        return t('errors.timeout')
      case 'PeerUnavailable':
        return t('errors.peerUnavailable')
      case 'PairingRejected':
        return t('errors.pairingRejected')
      case 'PairingFailed':
        return t('errors.pairingFailed')
      default:
        return t('errors.loadPeers')
    }
  }

  const errorMessage = resolveErrorMessage()

  const getIcon = (type: string) => {
    switch (type.toLowerCase()) {
      case 'mobile':
        return <Smartphone className="h-5 w-5" />
      case 'desktop':
        return <Monitor className="h-5 w-5" />
      default:
        return <Laptop className="h-5 w-5" />
    }
  }

  return (
    <StepLayout
      title={t('title')}
      subtitle={t('subtitle')}
      error={errorMessage}
      direction={direction}
    >
      <div className="mt-6 min-h-48 sm:mt-8">
        <AnimatePresence mode="wait">
          {scanPhase === 'scanning' && (
            <motion.div
              key="scanning-full"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0, scale: 0.95 }}
              transition={{ duration: 0.3 }}
            >
              <div className="flex flex-col items-center justify-center py-12 text-center sm:py-16">
                {/* Pulse ripple animation */}
                <div className="relative mb-6 flex items-center justify-center">
                  <div className="absolute h-24 w-24 rounded-full bg-primary/20 animate-ripple" />
                  <div className="absolute h-24 w-24 rounded-full bg-primary/20 animate-ripple [animation-delay:0.6s]" />
                  <div className="absolute h-24 w-24 rounded-full bg-primary/20 animate-ripple [animation-delay:1.2s]" />
                  <div className="relative z-10 flex h-16 w-16 items-center justify-center rounded-full bg-primary/10">
                    <Radar className="h-8 w-8 text-primary" />
                  </div>
                </div>
                <p className="text-foreground">{t('scanning.title')}</p>
                <p className="mt-2 max-w-sm text-sm text-muted-foreground">
                  {t('scanning.description')}
                </p>
              </div>
            </motion.div>
          )}

          {scanPhase === 'hasDevices' && (
            <motion.div
              key="scanning-compact"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.3 }}
            >
              {/* Compact scanning indicator */}
              <div className="mb-4 flex items-center gap-2 text-sm text-muted-foreground">
                <span className="relative flex h-2 w-2">
                  <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-primary/60" />
                  <span className="relative inline-flex h-2 w-2 rounded-full bg-primary" />
                </span>
                {t('scanning.compact')}
              </div>

              {/* Animated device list */}
              <div className="space-y-2">
                <AnimatePresence>
                  {peers.map(peer => (
                    <motion.div
                      key={peer.id}
                      initial={{ opacity: 0, y: -8 }}
                      animate={{ opacity: 1, y: 0 }}
                      exit={{ opacity: 0, y: 4, transition: { duration: 0.15 } }}
                      transition={{ duration: 0.2, ease: 'easeOut' }}
                      className="flex items-center gap-4 rounded-lg px-4 py-4 transition-colors hover:bg-muted/30"
                    >
                      <div className="flex h-10 w-10 items-center justify-center text-primary">
                        {getIcon(peer.device_type)}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="truncate font-medium">
                          {peer.deviceName || tCommon('unknownDevice')}
                        </div>
                        <div className="truncate font-mono text-xs text-muted-foreground">
                          {formatPeerIdForDisplay(peer.id)}
                        </div>
                      </div>
                      <Button size="sm" onClick={() => onSelectPeer(peer.id)} disabled={loading}>
                        {t('actions.select')}
                      </Button>
                    </motion.div>
                  ))}
                </AnimatePresence>
              </div>
            </motion.div>
          )}

          {scanPhase === 'empty' && (
            <motion.div
              key="empty-state"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.3 }}
            >
              <div className="flex flex-col items-center justify-center py-10 text-center sm:py-12">
                <p className="text-foreground">{t('empty.title')}</p>
                <p className="mt-2 max-w-sm text-sm text-muted-foreground">
                  {t('empty.description')}
                </p>

                {/* Troubleshooting tips */}
                <div className="mt-6 w-full max-w-sm rounded-lg border border-muted/50 bg-muted/20 p-4 text-left">
                  <p className="mb-2 text-sm font-medium text-foreground">
                    {t('empty.tips.heading')}
                  </p>
                  <ul className="space-y-1.5 text-sm text-muted-foreground">
                    <li>* {t('empty.tips.sameNetwork')}</li>
                    <li>* {t('empty.tips.pairingEnabled')}</li>
                    <li>* {t('empty.tips.firewall')}</li>
                  </ul>
                </div>

                <Button
                  variant="outline"
                  size="sm"
                  onClick={onRescan}
                  disabled={loading}
                  className="mt-6"
                >
                  <RefreshCw className={`mr-2 h-3 w-3 ${loading ? 'animate-spin' : ''}`} />
                  {t('empty.rescan')}
                </Button>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </StepLayout>
  )
}
