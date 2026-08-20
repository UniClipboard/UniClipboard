import { CircleCheck, RotateCcw, TriangleAlert, X } from 'lucide-react'
import { type MouseEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { resetSetup } from '@/api/daemon/setupV2'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('reset-space-danger-zone')

interface ResetSpaceDangerZoneProps {
  onResetComplete: () => void | Promise<void>
}

export default function ResetSpaceDangerZone({ onResetComplete }: ResetSpaceDangerZoneProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const busyRef = useRef(false)

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen && busyRef.current) return
    setOpen(nextOpen)
  }

  const handleConfirm = async (event: MouseEvent) => {
    event.preventDefault()
    if (busyRef.current) return
    busyRef.current = true
    setBusy(true)
    try {
      await resetSetup()
      await onResetComplete()
      setOpen(false)
      toast.success(t('devices.resetSpace.success'))
    } catch (error) {
      log.error({ err: error }, 'failed to reset space')
      toast.error(t('devices.resetSpace.error'))
    } finally {
      busyRef.current = false
      setBusy(false)
    }
  }

  return (
    <>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="text-destructive hover:bg-destructive/10 hover:text-destructive"
        aria-label={t('devices.resetSpace.action')}
        title={t('devices.resetSpace.action')}
        onClick={() => setOpen(true)}
      >
        <RotateCcw className="size-3.5" />
        <span className="hidden @lg:inline">{t('devices.resetSpace.action')}</span>
      </Button>

      <AlertDialog open={open} onOpenChange={handleOpenChange}>
        <AlertDialogContent className="sm:max-w-md">
          <AlertDialogHeader>
            <AlertDialogMedia className="bg-destructive/10 text-destructive">
              <TriangleAlert />
            </AlertDialogMedia>
            <AlertDialogTitle>{t('devices.resetSpace.confirmTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('devices.resetSpace.confirmDescription')}
            </AlertDialogDescription>
          </AlertDialogHeader>

          <div className="grid gap-3 text-sm">
            <div className="rounded-md border border-destructive/20 bg-destructive/5 p-3">
              <p className="font-medium text-destructive">{t('devices.resetSpace.removedTitle')}</p>
              <ul className="mt-2 grid gap-1.5 text-xs leading-relaxed text-foreground/80">
                <li className="flex gap-2">
                  <X className="mt-0.5 size-3.5 shrink-0 text-destructive" />
                  {t('devices.resetSpace.removedRelationships')}
                </li>
                <li className="flex gap-2">
                  <X className="mt-0.5 size-3.5 shrink-0 text-destructive" />
                  {t('devices.resetSpace.removedPairing')}
                </li>
              </ul>
            </div>
            <div className="rounded-md border border-success/20 bg-success/5 p-3">
              <p className="font-medium text-success">{t('devices.resetSpace.keptTitle')}</p>
              <p className="mt-2 flex gap-2 text-xs leading-relaxed text-foreground/80">
                <CircleCheck className="mt-0.5 size-3.5 shrink-0 text-success" />
                {t('devices.resetSpace.keptContent')}
              </p>
            </div>
          </div>

          <AlertDialogFooter>
            <AlertDialogCancel disabled={busy}>{t('devices.resetSpace.cancel')}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" disabled={busy} onClick={handleConfirm}>
              {busy ? t('devices.resetSpace.resetting') : t('devices.resetSpace.confirmAction')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
