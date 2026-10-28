import React, { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getPairedPeers } from '@/api/daemon/members'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { answerResendConfirmation, useResendConfirmationRequest } from '@/hooks/resendConfirmation'
import { createLogger } from '@/lib/logger'

const log = createLogger('resend-confirm-dialog')

function shortenId(id: string): string {
  return id.length <= 10 ? id : `${id.slice(0, 8)}…`
}

/**
 * Asks once before a resend starts a new transfer. Declining (Esc, Cancel,
 * closing) never starts anything: the resend then only reports what the
 * reconciliation with the existing task found.
 */
const ResendConfirmDialog: React.FC = () => {
  const { t } = useTranslation()
  const request = useResendConfirmationRequest()
  const [names, setNames] = useState<Record<string, string>>({})

  useEffect(() => {
    if (!request) return
    let cancelled = false
    getPairedPeers()
      .then(peers => {
        if (cancelled) return
        setNames(Object.fromEntries(peers.map(peer => [peer.peerId, peer.deviceName])))
      })
      .catch(err => {
        // Names are cosmetic; fall back to shortened ids.
        log.warn({ err }, 'failed to load device names for resend confirmation')
      })
    return () => {
      cancelled = true
    }
  }, [request])

  const devices = (request?.targetDeviceIds ?? []).map(id => names[id] || shortenId(id))

  return (
    <AlertDialog
      open={request !== null}
      onOpenChange={open => {
        if (!open) answerResendConfirmation(false)
      }}
    >
      <AlertDialogContent data-testid="resend-confirm-dialog">
        <AlertDialogHeader>
          <AlertDialogTitle>{t('delivery.resend.confirm.title')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('delivery.resend.confirm.description', {
              count: request?.entryCount ?? 0,
              devices: devices.join(', '),
              deviceCount: devices.length,
            })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {request?.duplicateFilesPossible && (
          <p className="text-ui-body text-muted-foreground">
            {t('delivery.resend.confirm.duplicateWarning')}
          </p>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel onClick={() => answerResendConfirmation(false)}>
            {t('delivery.resend.confirm.cancel')}
          </AlertDialogCancel>
          <AlertDialogAction
            data-testid="resend-confirm-accept"
            onClick={e => {
              e.preventDefault()
              answerResendConfirmation(true)
            }}
          >
            {t('delivery.resend.confirm.accept')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

export default ResendConfirmDialog
