import { Eye, EyeOff, Loader2, ArrowLeft } from 'lucide-react'
import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { JoinVerifyPassphraseStepProps } from './types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { formatPeerIdForDisplay } from '@/lib/utils'
import StepLayout from '@/pages/setup/StepLayout'

export default function JoinVerifyPassphraseStep({
  peerId,
  onSubmit,
  onBack,
  onCreateNew,
  error,
  loading,
  direction,
}: JoinVerifyPassphraseStepProps) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.joinVerifyPassphrase' })
  const { t: tCommon } = useTranslation(undefined, { keyPrefix: 'setup.common' })

  const [passphrase, setPassphrase] = useState('')
  const [showPassphrase, setShowPassphrase] = useState(false)
  const [localError, setLocalError] = useState<string | null>(null)
  const [showMismatchHelp, setShowMismatchHelp] = useState(true)

  useEffect(() => {
    if (!error) {
      setLocalError(null)
      setShowMismatchHelp(true)
      return
    }
    setShowMismatchHelp(true)
    if (error === 'PassphraseInvalidOrMismatch') {
      setLocalError(null)
    } else if (error === 'NetworkTimeout') {
      setLocalError(t('errors.timeout'))
    } else if (error === 'PeerUnavailable') {
      setLocalError(t('errors.peerUnavailable'))
    } else {
      setLocalError(t('errors.generic'))
    }
  }, [error, t])

  const handleSubmit = () => {
    if (!passphrase) {
      setLocalError(t('errors.empty'))
      return
    }
    onSubmit(passphrase)
  }

  const backButton = (
    <button
      type="button"
      onClick={onBack}
      className="flex items-center gap-1 text-sm text-muted-foreground transition-colors hover:text-foreground"
    >
      <ArrowLeft className="h-4 w-4" />
      {t('actions.backToPick')}
    </button>
  )

  if (error === 'PassphraseInvalidOrMismatch' && showMismatchHelp) {
    return (
      <StepLayout
        headerLeft={backButton}
        title={t('mismatchHelp.title')}
        subtitle={t('mismatchHelp.subtitle')}
        direction={direction}
      >
        <div className="mt-5 space-y-5 sm:mt-6 sm:space-y-6">
          <div className="text-sm text-muted-foreground">
            <p>{t('mismatchHelp.p1')}</p>
            <p className="mt-2">{t('mismatchHelp.p2')}</p>
            <ul className="mt-2 list-disc space-y-1 pl-5">
              <li>{t('mismatchHelp.option1')}</li>
              <li>{t('mismatchHelp.option2')}</li>
            </ul>
          </div>

          <div className="flex flex-col gap-3 pt-3 sm:pt-4">
            <Button onClick={() => setShowMismatchHelp(false)} disabled={loading}>
              {t('mismatchHelp.retry')}
            </Button>
            <Button variant="outline" onClick={onCreateNew} disabled={loading}>
              {t('mismatchHelp.createNew')}
            </Button>
          </div>
        </div>
      </StepLayout>
    )
  }

  const verifyButton = (
    <div className="flex items-center gap-4">
      <Button onClick={handleSubmit} disabled={loading} className="min-w-32">
        {loading ? (
          <>
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            {t('actions.verifying')}
          </>
        ) : (
          t('actions.verify')
        )}
      </Button>
    </div>
  )

  return (
    <StepLayout
      headerLeft={backButton}
      title={t('title')}
      subtitle={t('subtitle')}
      error={localError}
      footer={verifyButton}
      direction={direction}
    >
      {peerId && (
        <p className="mt-1 font-mono text-xs text-muted-foreground">
          {t('targetDevice', { peerShort: formatPeerIdForDisplay(peerId) })}
        </p>
      )}
      <div className="mt-6 space-y-6 sm:mt-8">
        <div className="space-y-2">
          <Label htmlFor="passphrase">{tCommon('encryptPassphraseLabel')}</Label>
          <div className="relative">
            <Input
              id="passphrase"
              type={showPassphrase ? 'text' : 'password'}
              value={passphrase}
              onChange={e => setPassphrase(e.target.value)}
              disabled={loading}
              className="pr-10"
              placeholder={tCommon('encryptPassphrasePlaceholder')}
              onKeyDown={e => e.key === 'Enter' && handleSubmit()}
            />
            <button
              type="button"
              onClick={() => setShowPassphrase(!showPassphrase)}
              className="absolute right-0 top-0 flex h-full items-center px-3 text-muted-foreground transition-colors hover:text-foreground"
            >
              {showPassphrase ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
            </button>
          </div>
        </div>
      </div>
    </StepLayout>
  )
}
