import { Lock, Unlock, Loader2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { unlockEncryptionSession, verifyKeychainAccess } from '@/api/security'
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { usePlatform } from '@/hooks/usePlatform'
import { useSetting } from '@/hooks/useSetting'

export default function UnlockPage() {
  const { t } = useTranslation()
  const { setting, updateSecuritySetting, loading: settingsLoading } = useSetting()
  const { isMac } = usePlatform()
  const [unlocking, setUnlocking] = useState(false)
  const [isExiting, setIsExiting] = useState(false)
  const [showKeychainModal, setShowKeychainModal] = useState(false)
  const [verifying, setVerifying] = useState(false)
  const [verifyError, setVerifyError] = useState<string | null>(null)

  const handleUnlock = async () => {
    setUnlocking(true)
    setIsExiting(false)
    try {
      const unlocked = await unlockEncryptionSession()
      if (unlocked) {
        // The App component will handle the navigation when the session becomes ready
        // via the encryption://event SessionReady listener.
        setIsExiting(true)
      } else {
        // unlock_encryption_session returned false — encryption was not initialized
        // or the session was already ready. Do not animate out; reset state.
        console.warn('Unlock returned false — encryption may not be initialized')
        setUnlocking(false)
        setIsExiting(false)
      }
    } catch (error) {
      console.error('Unlock failed:', error)
      setUnlocking(false)
      setIsExiting(false)
    }
  }

  const handleAutoUnlockChange = async (checked: boolean) => {
    if (checked && isMac) {
      setVerifyError(null)
      setShowKeychainModal(true)
      return
    }
    await updateSecuritySetting({ auto_unlock_enabled: checked })
  }

  const handleKeychainVerify = async () => {
    setVerifying(true)
    setVerifyError(null)
    try {
      const granted = await verifyKeychainAccess()
      if (granted) {
        await updateSecuritySetting({ auto_unlock_enabled: true })
        setShowKeychainModal(false)
      } else {
        setVerifyError(t('unlock.keychainModal.error'))
      }
    } catch {
      setVerifyError(t('unlock.keychainModal.error'))
    } finally {
      setVerifying(false)
    }
  }

  const handleKeychainCancel = () => {
    setShowKeychainModal(false)
    setVerifyError(null)
  }

  return (
    <div className="relative flex min-h-screen w-full flex-col items-center justify-center overflow-hidden bg-background p-4">
      <div className="absolute inset-0 bg-gradient-to-b from-background via-background to-muted/20" />

      <div className="absolute -top-24 -left-16 h-96 w-96 rounded-full bg-primary/5 blur-3xl" />
      <div className="absolute -bottom-24 -right-16 h-96 w-96 rounded-full bg-primary/5 blur-3xl" />

      <div
        className={`relative z-10 flex w-full max-w-sm flex-col items-center space-y-8 text-center transition-all duration-500 ${
          isExiting ? 'translate-y-2 opacity-0' : 'translate-y-0 opacity-100'
        }`}
      >
        <div className="relative flex h-24 w-24 items-center justify-center rounded-3xl bg-muted/30 shadow-inner ring-1 ring-border/50">
          <div className="absolute inset-0 rounded-3xl bg-gradient-to-br from-primary/10 to-transparent opacity-50" />
          <Lock className="h-10 w-10 text-primary" />
        </div>

        <div className="space-y-2">
          <h1 className="text-3xl font-bold tracking-tight text-foreground sm:text-4xl">
            {t('unlock.title')}
          </h1>
          <p className="text-muted-foreground">{t('unlock.description')}</p>
        </div>

        <div className="w-full space-y-6">
          <Button
            size="lg"
            className="h-12 w-full rounded-xl text-base font-medium shadow-lg shadow-primary/20 transition-all hover:scale-[1.02] hover:shadow-primary/30"
            onClick={handleUnlock}
            disabled={unlocking}
          >
            {unlocking ? (
              <>
                <Loader2 className="mr-2 h-5 w-5 animate-spin" />
                {t('unlock.unlocking')}
              </>
            ) : (
              <>
                <Unlock className="mr-2 h-5 w-5" />
                {t('unlock.button')}
              </>
            )}
          </Button>

          <div className="flex items-center justify-between rounded-xl border border-border/40 bg-muted/20 px-4 py-3 backdrop-blur-sm transition-colors hover:bg-muted/30">
            <div className="flex flex-col items-start space-y-0.5 text-left">
              <Label htmlFor="auto-unlock" className="cursor-pointer text-sm font-medium">
                {t('unlock.autoUnlock.label')}
              </Label>
              <span className="text-xs text-muted-foreground">
                {t('unlock.autoUnlock.description')}
              </span>
            </div>
            <Switch
              id="auto-unlock"
              checked={setting?.security?.auto_unlock_enabled ?? false}
              onCheckedChange={handleAutoUnlockChange}
              disabled={settingsLoading}
            />
          </div>
        </div>

        {isMac && (
          <p className="max-w-xs text-xs text-muted-foreground/60">{t('unlock.macOSNote')}</p>
        )}
      </div>

      <AlertDialog open={showKeychainModal}>
        <AlertDialogContent onEscapeKeyDown={e => e.preventDefault()}>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('unlock.keychainModal.title')}</AlertDialogTitle>
            <AlertDialogDescription>{t('unlock.keychainModal.description')}</AlertDialogDescription>
          </AlertDialogHeader>

          <ol className="list-decimal space-y-2 pl-5 text-sm text-foreground">
            <li>{t('unlock.keychainModal.step1')}</li>
            <li>{t('unlock.keychainModal.step2')}</li>
            <li>{t('unlock.keychainModal.step3')}</li>
          </ol>

          <p className="text-xs text-muted-foreground">{t('unlock.keychainModal.note')}</p>

          {verifyError && (
            <div className="rounded-lg border border-destructive/20 bg-destructive/5 p-3">
              <p className="text-sm font-medium text-destructive">{verifyError}</p>
            </div>
          )}

          <AlertDialogFooter>
            <Button variant="outline" onClick={handleKeychainCancel} disabled={verifying}>
              {t('unlock.keychainModal.cancel')}
            </Button>
            <Button variant="secondary" onClick={handleKeychainVerify} disabled={verifying}>
              {verifying ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  {t('unlock.keychainModal.verifying')}
                </>
              ) : (
                t('unlock.keychainModal.confirm')
              )}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
