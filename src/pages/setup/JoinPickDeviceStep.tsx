import { ArrowLeft, RefreshCw, Monitor, Smartphone, Laptop } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { JoinPickDeviceStepProps } from './types'
import { Button } from '@/components/ui/button'
import { formatPeerIdForDisplay } from '@/lib/utils'
import StepLayout from '@/pages/setup/StepLayout'

export default function JoinPickDeviceStep({
  onSelectPeer,
  onBack,
  onRefresh,
  peers,
  error,
  loading,
  isScanningInitial,
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

  const backButton = (
    <button
      type="button"
      onClick={onBack}
      className="flex items-center gap-1 text-sm text-muted-foreground transition-colors hover:text-foreground"
    >
      <ArrowLeft className="h-4 w-4" />
      {tCommon('back')}
    </button>
  )

  const refreshButton = (
    <button
      type="button"
      onClick={onRefresh}
      disabled={loading}
      className="flex items-center gap-1 text-sm text-muted-foreground transition-colors hover:text-foreground disabled:opacity-50"
    >
      <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
      {tCommon('refresh')}
    </button>
  )

  return (
    <StepLayout
      headerLeft={backButton}
      headerRight={refreshButton}
      title={t('title')}
      subtitle={t('subtitle')}
      error={errorMessage}
      direction={direction}
    >
      <div className="mt-6 min-h-48 space-y-2 sm:mt-8">
        {isScanningInitial ? (
          <div className="flex flex-col items-center justify-center py-10 text-center sm:py-12">
            <RefreshCw className="mb-4 h-8 w-8 animate-spin text-muted-foreground" />
            <p className="text-foreground">{t('scanning.title')}</p>
            <p className="mt-2 max-w-sm text-sm text-muted-foreground">
              {t('scanning.description')}
            </p>
          </div>
        ) : peers.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-10 text-center sm:py-12">
            <p className="text-foreground">{t('empty.title')}</p>
            <p className="mt-2 max-w-sm text-sm text-muted-foreground">{t('empty.description')}</p>
            <Button
              variant="outline"
              size="sm"
              onClick={onRefresh}
              disabled={loading}
              className="mt-6"
            >
              <RefreshCw className={`mr-2 h-3 w-3 ${loading ? 'animate-spin' : ''}`} />
              {t('empty.rescan')}
            </Button>
          </div>
        ) : (
          peers.map((peer: JoinPickDeviceStepProps['peers'][number]) => (
            <div
              key={peer.id}
              className="flex items-center gap-4 rounded-lg px-4 py-4 transition-colors hover:bg-muted/30"
            >
              <div className="flex h-10 w-10 items-center justify-center text-primary">
                {getIcon(peer.device_type)}
              </div>
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium">{peer.name}</div>
                <div className="truncate font-mono text-xs text-muted-foreground">
                  {formatPeerIdForDisplay(peer.id)}
                </div>
              </div>
              <Button size="sm" onClick={() => onSelectPeer(peer.id)} disabled={loading}>
                {t('actions.select')}
              </Button>
            </div>
          ))
        )}
      </div>
    </StepLayout>
  )
}
