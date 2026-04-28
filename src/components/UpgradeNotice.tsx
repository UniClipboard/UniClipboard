import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { acknowledgeUpgrade, type UpgradeStatus } from '@/api/daemon'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui'
import { createLogger } from '@/lib/logger'

const log = createLogger('upgrade-notice')

interface UpgradeNoticeProps {
  /** Status fetched from the daemon. The dialog shows only when
   *  `kind === 'upgraded' && from === null` (old user crossing the pre-cursor
   *  boundary). Other variants render nothing. */
  status: UpgradeStatus | null
  /** Invoked after the user dismisses. `acknowledged = true` when the cursor
   *  was advanced (Got it), `false` when the user chose Later. */
  onDismiss: (acknowledged: boolean) => void
}

/**
 * Re-pair guidance dialog shown after an upgrade across the breaking pairing
 * change. Only the "old user from pre-cursor era" variant qualifies in P1;
 * other upgrade kinds (`upgraded` with a known `from`, `downgraded`,
 * `no_change`, `fresh_install`) render nothing here.
 */
export default function UpgradeNotice({ status, onDismiss }: UpgradeNoticeProps) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)

  const shouldShow = status?.kind === 'upgraded' && status.from === null
  if (!shouldShow) return null

  const handleGotIt = async () => {
    setBusy(true)
    try {
      await acknowledgeUpgrade()
      onDismiss(true)
    } catch (error) {
      log.error({ err: error }, 'Failed to acknowledge upgrade')
      // Treat as dismissed-but-unack: user will see the modal again next launch.
      onDismiss(false)
    } finally {
      setBusy(false)
    }
  }

  const handleLater = () => {
    onDismiss(false)
  }

  const toLine = status.kind === 'upgraded' ? t('upgradeNotice.toVersion', { to: status.to }) : null

  return (
    <AlertDialog open={true}>
      <AlertDialogContent className="bg-card text-card-foreground">
        <AlertDialogHeader>
          <AlertDialogTitle>{t('upgradeNotice.title')}</AlertDialogTitle>
          <AlertDialogDescription className="space-y-2">
            <span className="block">{t('upgradeNotice.body')}</span>
            <span className="text-muted-foreground block text-xs">
              {t('upgradeNotice.fromUnknown')}
              {toLine ? ` · ${toLine}` : null}
            </span>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={busy} onClick={handleLater}>
            {t('upgradeNotice.later')}
          </AlertDialogCancel>
          <AlertDialogAction disabled={busy} onClick={handleGotIt}>
            {t('upgradeNotice.gotIt')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
